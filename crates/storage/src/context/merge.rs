use sea_orm::TransactionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    context_merge::{MergeContextCommand, MergeContextStatus, MergeContextView, analyze_three_way},
};

use crate::{SqliteStore, StorageError, StorageResult};

use super::{
    merge_append::apply_verified_appends,
    merge_append_history::load_append_history,
    merge_base::{load_heads, unique_merge_base},
    merge_commit::commit_merge,
    merge_conflict::{MergeIdentity, persist_conflicts, resolve},
    merge_receipt::{finish_receipt, replay},
    merge_selection::validate_selections,
    replay::reconstruct,
};

impl SqliteStore {
    pub async fn merge_context_at(
        &self,
        command: MergeContextCommand,
        now: i64,
    ) -> StorageResult<MergeContextView> {
        validate(&command)?;
        let scope = format!(
            "context:merges:{}:{}:{}",
            command.context_id, command.source_branch_id, command.target_branch_id
        );
        let digest = canonical::hash(&json!({
            "schemaVersion":1,"command":"merge_context","contextId":command.context_id,
            "sourceBranchId":command.source_branch_id,"targetBranchId":command.target_branch_id,
            "expectedSourceHead":command.expected_source_head,"expectedTargetHead":command.expected_target_head,
            "sourceDisposition":command.source_disposition,"selections":command.selections,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(result) =
            replay(&transaction, &scope, &command.idempotency_key, &digest).await?
        {
            transaction.commit().await?;
            return Ok(result);
        }
        let heads = load_heads(
            &transaction,
            &command.context_id,
            &command.source_branch_id,
            &command.target_branch_id,
        )
        .await?;
        if heads.source != command.expected_source_head
            || heads.target != command.expected_target_head
        {
            return Err(StorageError::Conflict("context_head"));
        }
        if heads.source == heads.target {
            return Err(StorageError::Conflict("merge_heads_identical"));
        }
        let base_commit_id = unique_merge_base(&transaction, &heads.source, &heads.target).await?;
        let base = reconstruct(&transaction, &base_commit_id).await?;
        let source = reconstruct(&transaction, &heads.source).await?;
        let target = reconstruct(&transaction, &heads.target).await?;
        if [
            base.context_id.as_str(),
            source.context_id.as_str(),
            target.context_id.as_str(),
        ]
        .iter()
        .any(|context| *context != command.context_id)
        {
            return Err(StorageError::Conflict("merge_context_mismatch"));
        }
        let source_history = load_append_history(
            &transaction,
            &heads.source,
            &base_commit_id,
            &base.append_ids,
        )
        .await?;
        let target_history = load_append_history(
            &transaction,
            &heads.target,
            &base_commit_id,
            &base.append_ids,
        )
        .await?;
        let mut analysis = analyze_three_way(&base.value, &source.value, &target.value);
        let verified_appends = apply_verified_appends(
            &mut analysis,
            &base.value,
            &source.value,
            &target.value,
            &source_history,
            &target_history,
        )?;
        let identity = MergeIdentity {
            context_id: &command.context_id,
            source_branch_id: &command.source_branch_id,
            target_branch_id: &command.target_branch_id,
            base_commit_id: &base_commit_id,
            source_head: &heads.source,
            target_head: &heads.target,
        };
        let conflicts =
            persist_conflicts(&transaction, &identity, &analysis.conflicts, now).await?;
        let resolved = resolve(
            analysis.merged,
            &conflicts,
            &command.selections,
            verified_appends.entries,
            verified_appends.blocked_paths,
        )?;
        validate_selections(&transaction, &command.context_id, &command.selections).await?;
        let (status, remaining, merge_commit_id) = match resolved {
            Ok(resolved) => {
                let commit_id =
                    commit_merge(&transaction, &command, &base_commit_id, &resolved, now).await?;
                (MergeContextStatus::Merged, vec![], Some(commit_id))
            }
            Err(missing) => (MergeContextStatus::Conflicted, missing, None),
        };
        let result = MergeContextView {
            context_id: command.context_id,
            source_branch_id: command.source_branch_id,
            target_branch_id: command.target_branch_id,
            base_commit_id,
            source_head_commit_id: heads.source,
            target_head_commit_id: heads.target,
            status,
            conflicts: remaining,
            merge_commit_id,
        };
        finish_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &result,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

fn validate(command: &MergeContextCommand) -> StorageResult<()> {
    if command.context_id.is_empty()
        || command.source_branch_id.is_empty()
        || command.target_branch_id.is_empty()
        || command.expected_source_head.is_empty()
        || command.expected_target_head.is_empty()
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
        || command.selections.len() > 256
    {
        return Err(StorageError::InvalidArgument(
            "invalid merge context command".into(),
        ));
    }
    Ok(())
}
