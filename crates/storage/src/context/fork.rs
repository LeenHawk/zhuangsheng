use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    runtime::{ContextBranchView, ForkContextCommand},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, sql},
};

use super::{
    fork_support::{append_event, finish_receipt, is_reachable, replay, verify_replay},
    replay::reconstruct,
};

impl SqliteStore {
    pub async fn fork_context_at(
        &self,
        command: ForkContextCommand,
        now: i64,
    ) -> StorageResult<ContextBranchView> {
        validate(&command)?;
        let scope = format!("context:branches:{}", command.context_id);
        let digest = canonical::hash(&json!({
            "schemaVersion":1,"command":"fork_context","contextId":command.context_id,
            "sourceBranchId":command.source_branch_id,"fromCommitId":command.from_commit_id,
            "expectedSourceHead":command.expected_source_head,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(result) =
            replay(&transaction, &scope, &command.idempotency_key, &digest).await?
        {
            verify_replay(&transaction, &result).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let source = transaction.query_one_raw(sql(
            "SELECT head_commit_id, status FROM context_branches WHERE context_id = ? AND id = ?",
            vec![command.context_id.clone().into(), command.source_branch_id.clone().into()],
        )).await?.ok_or_else(|| StorageError::NotFound { kind: "context_branch", id: command.source_branch_id.clone() })?;
        let source_head: String = source.try_get("", "head_commit_id")?;
        if source.try_get::<String>("", "status")? != "active" {
            return Err(StorageError::Conflict("source_branch_not_active"));
        }
        if command
            .expected_source_head
            .as_ref()
            .is_some_and(|head| head != &source_head)
        {
            return Err(StorageError::Conflict("context_head"));
        }
        if !is_reachable(&transaction, &source_head, &command.from_commit_id).await? {
            return Err(StorageError::Conflict("fork_commit_not_reachable"));
        }
        let result = fork_branch_at_commit(
            &transaction,
            &command.context_id,
            &command.source_branch_id,
            &command.from_commit_id,
            now,
        )
        .await?;
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

pub(crate) async fn fork_branch_at_commit<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    source_branch_id: &str,
    from_commit_id: &str,
    now: i64,
) -> StorageResult<ContextBranchView> {
    let reconstructed = reconstruct(connection, from_commit_id).await?;
    if reconstructed.context_id != context_id {
        return Err(StorageError::Conflict("fork_context_mismatch"));
    }
    let branch_id = new_id("branch");
    connection.execute_raw(sql(
        "INSERT INTO context_branches (id, context_id, parent_branch_id, fork_commit_id, head_commit_id, creation_operation_id, status, pinned, audit_hold, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'active', 0, 0, ?, ?)",
        vec![branch_id.clone().into(), context_id.into(), source_branch_id.into(), from_commit_id.into(), from_commit_id.into(), format!("fork-context:{branch_id}").into(), now.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO materialized_projections (aggregate_kind, aggregate_id, lineage_key, head_commit_id, projection_json, schema_version, updated_at) VALUES ('working_context', ?, ?, ?, ?, 1, ?)",
        vec![context_id.into(), branch_id.clone().into(), from_commit_id.into(), canonical::to_string(&reconstructed.value)?.into(), now.into()],
    )).await?;
    append_event(
        connection,
        context_id,
        &branch_id,
        source_branch_id,
        from_commit_id,
        now,
    )
    .await?;
    Ok(ContextBranchView {
        context_id: context_id.into(),
        branch_id,
        head_commit_id: from_commit_id.into(),
        fork_commit_id: from_commit_id.into(),
        status: "active".into(),
    })
}

fn validate(command: &ForkContextCommand) -> StorageResult<()> {
    if command.context_id.is_empty()
        || command.source_branch_id.is_empty()
        || command.from_commit_id.is_empty()
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid fork context command".into(),
        ));
    }
    Ok(())
}
