use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::SelectConversationCandidateCommand, canonical,
    conversation::ConversationSelectionView,
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::{
    events::append_event,
    receipt::{Receipt, finish, replay},
};

impl SqliteStore {
    pub async fn select_conversation_candidate_at(
        &self,
        command: SelectConversationCandidateCommand,
        now: i64,
    ) -> StorageResult<ConversationSelectionView> {
        validate(&command)?;
        let scope = format!("conversation:turn-selection:{}", command.turn_id);
        let digest = canonical::hash(&json!({
            "schemaVersion":1,"command":"select_conversation_candidate",
            "turnId":command.turn_id,"selectedRunId":command.selected_run_id,
            "expectedConversationHeadCommitId":command.expected_conversation_head_commit_id,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(result) = replay::<_, ConversationSelectionView>(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(result);
        }
        let row = transaction.query_one_raw(sql(
            "SELECT t.conversation_id, t.user_commit_id, c.context_id, tc.branch_id, tc.base_commit_id, tc.candidate_commit_id, tc.status, b.context_id AS branch_context, b.head_commit_id AS branch_head, vc.aggregate_id AS commit_context, vc.lineage_key AS commit_branch FROM turn_candidates tc JOIN conversation_turns t ON t.id = tc.turn_id JOIN conversations c ON c.id = t.conversation_id JOIN context_branches b ON b.id = tc.branch_id LEFT JOIN version_commits vc ON vc.id = tc.candidate_commit_id AND vc.aggregate_kind = 'working_context' WHERE tc.turn_id = ? AND tc.run_id = ?",
            vec![command.turn_id.clone().into(), command.selected_run_id.clone().into()],
        )).await?.ok_or_else(|| StorageError::NotFound { kind: "turn_candidate", id: command.selected_run_id.clone() })?;
        if row.try_get::<String>("", "status")? != "ready" {
            return Err(StorageError::Conflict("candidate_not_ready"));
        }
        let conversation_id: String = row.try_get("", "conversation_id")?;
        let context_id: String = row.try_get("", "context_id")?;
        let branch_id: String = row.try_get("", "branch_id")?;
        let commit_id: String = row.try_get("", "candidate_commit_id")?;
        if row.try_get::<String>("", "base_commit_id")?
            != row.try_get::<String>("", "user_commit_id")?
            || row.try_get::<String>("", "branch_context")? != context_id
            || row.try_get::<String>("", "branch_head")? != commit_id
            || row.try_get::<String>("", "commit_context")? != context_id
            || row.try_get::<String>("", "commit_branch")? != branch_id
        {
            return Err(StorageError::Integrity(
                "ready candidate lineage is corrupt".into(),
            ));
        }
        let updated = transaction.execute_raw(sql(
            "UPDATE conversations SET active_branch_id = ?, active_head_commit_id = ?, updated_at = ? WHERE id = ? AND active_head_commit_id = ?",
            vec![branch_id.clone().into(), commit_id.clone().into(), now.into(), conversation_id.clone().into(), command.expected_conversation_head_commit_id.clone().into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("conversation_head"));
        }
        transaction.execute_raw(sql(
            "INSERT INTO conversation_selections (turn_id, selected_run_id, selection_scope, selection_key, selected_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(turn_id) DO UPDATE SET selected_run_id = excluded.selected_run_id, selection_scope = excluded.selection_scope, selection_key = excluded.selection_key, selected_at = excluded.selected_at",
            vec![command.turn_id.clone().into(), command.selected_run_id.clone().into(), scope.clone().into(), command.idempotency_key.clone().into(), now.into()],
        )).await?;
        let result = ConversationSelectionView {
            turn_id: command.turn_id,
            selected_run_id: command.selected_run_id,
            selected_branch_id: branch_id,
            selected_commit_id: commit_id,
            selected_at: now,
        };
        append_event(&transaction, &conversation_id, "conversation.selection_changed", &json!({"schemaVersion":1,"turnId":result.turn_id,"selectedRunId":result.selected_run_id,"selectedCommitId":result.selected_commit_id,"mode":"explicit"}), now).await?;
        finish(
            &transaction,
            Receipt {
                scope: &scope,
                key: &command.idempotency_key,
                digest: &digest,
                command_kind: "conversation.candidate.select",
                resource_kind: "conversation_selection",
                resource_id: &result.turn_id,
                now,
            },
            &result,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn auto_select<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    turn_id: &str,
    run_id: &str,
    branch_id: &str,
    base_commit_id: &str,
    candidate_commit_id: &str,
    now: i64,
) -> StorageResult<bool> {
    let updated = connection.execute_raw(sql(
        "UPDATE conversations SET active_branch_id = ?, active_head_commit_id = ?, updated_at = ? WHERE id = ? AND active_head_commit_id = ? AND NOT EXISTS (SELECT 1 FROM conversation_selections WHERE turn_id = ?)",
        vec![branch_id.into(), candidate_commit_id.into(), now.into(), conversation_id.into(), base_commit_id.into(), turn_id.into()],
    )).await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    connection.execute_raw(sql(
        "INSERT INTO conversation_selections (turn_id, selected_run_id, selection_scope, selection_key, selected_at) VALUES (?, ?, ?, ?, ?)",
        vec![turn_id.into(), run_id.into(), format!("conversation:auto-selection:{turn_id}").into(), run_id.into(), now.into()],
    )).await?;
    append_event(connection, conversation_id, "conversation.selection_changed", &json!({"schemaVersion":1,"turnId":turn_id,"selectedRunId":run_id,"selectedCommitId":candidate_commit_id,"mode":"auto_first_ready"}), now).await?;
    Ok(true)
}

fn validate(command: &SelectConversationCandidateCommand) -> StorageResult<()> {
    if command.turn_id.is_empty()
        || command.selected_run_id.is_empty()
        || command.expected_conversation_head_commit_id.is_empty()
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid candidate selection command".into(),
        ));
    }
    Ok(())
}
