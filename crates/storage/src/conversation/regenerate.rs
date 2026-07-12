use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::{
        RegenerateConversationCandidateCommand, RegenerateConversationCandidateResult,
    },
    canonical,
    conversation::{
        ConversationRunInputV1, ConversationTurnView, TurnCandidateStatus, TurnCandidateView,
    },
    llm::ir::{LlmContentPartIr, validate_content_parts},
    runtime::{RunContextCommand, StartRunCommand},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    context::replay::reconstruct,
    graph::helpers::{load_object_json, new_id, sql},
    runtime::{query::load_run, start_insert::insert_new_run},
};

use super::{
    contract::validate_run_spec,
    events::append_event,
    receipt::{Receipt, finish, replay},
    submit_rows::{fork_candidate_from_commit, insert_candidate},
};

impl SqliteStore {
    pub async fn regenerate_conversation_candidate_at(
        &self,
        command: RegenerateConversationCandidateCommand,
        now: i64,
    ) -> StorageResult<RegenerateConversationCandidateResult> {
        validate(&command)?;
        let scope = format!("conversation:turn-regenerations:{}", command.turn_id);
        let digest = canonical::hash(&json!({
            "schemaVersion":1,"command":"regenerate_conversation_candidate",
            "turnId":command.turn_id,"expectedUserCommitId":command.expected_user_commit_id,
            "run":command.run,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(result) = replay::<_, RegenerateConversationCandidateResult>(
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
        let turn = load_turn(&transaction, &command.turn_id).await?;
        if turn.view.user_commit_id != command.expected_user_commit_id {
            return Err(StorageError::Conflict("conversation_user_commit"));
        }
        validate_run_spec(&transaction, &command.run).await?;
        let content: Vec<LlmContentPartIr> =
            load_object_json(&transaction, &turn.content_id).await?;
        validate_content_parts(&content, true)
            .map_err(|_| StorageError::Integrity("stored user content is invalid".into()))?;
        let run_id = new_id("run");
        let branch_id = new_id("branch");
        let reconstructed = reconstruct(&transaction, &turn.view.user_commit_id).await?;
        if reconstructed.context_id != turn.context_id {
            return Err(StorageError::Integrity(
                "turn user commit crossed context boundary".into(),
            ));
        }
        fork_candidate_from_commit(
            &transaction,
            &turn.context_id,
            &turn.user_branch_id,
            &branch_id,
            &turn.view.user_commit_id,
            &run_id,
            &reconstructed.value,
            now,
        )
        .await?;
        let input = ConversationRunInputV1 {
            schema_version: 1,
            conversation_id: turn.view.conversation_id.clone(),
            turn_id: turn.view.id.clone(),
            user_message_id: turn.view.user_message_id.clone(),
            user_commit_id: turn.view.user_commit_id.clone(),
            content,
        };
        insert_new_run(
            &transaction,
            &StartRunCommand {
                graph_revision_id: command.run.graph_revision_id,
                input: serde_json::to_value(input)
                    .map_err(|error| StorageError::Integrity(error.to_string()))?,
                context: RunContextCommand::Existing {
                    context_id: turn.context_id,
                    branch_id: branch_id.clone(),
                    expected_head_commit_id: turn.view.user_commit_id.clone(),
                },
                deadline_at: None,
                idempotency_key: format!("conversation-candidate:{run_id}"),
            },
            &run_id,
            now,
        )
        .await?;
        let candidate = TurnCandidateView {
            turn_id: turn.view.id.clone(),
            run_id: run_id.clone(),
            branch_id,
            base_commit_id: turn.view.user_commit_id.clone(),
            reply_output_key: command.run.reply_output_key,
            status: TurnCandidateStatus::Running,
            created_at: now,
        };
        insert_candidate(
            &transaction,
            &turn.view,
            &candidate,
            &scope,
            &command.idempotency_key,
        )
        .await?;
        let result = RegenerateConversationCandidateResult {
            candidate,
            run: load_run(&transaction, &run_id).await?,
        };
        append_event(
            &transaction,
            &turn.view.conversation_id,
            "conversation.candidate_regenerated",
            &json!({"schemaVersion":1,"turnId":turn.view.id,"runId":run_id}),
            now,
        )
        .await?;
        finish(
            &transaction,
            Receipt {
                scope: &scope,
                key: &command.idempotency_key,
                digest: &digest,
                command_kind: "conversation.candidate.regenerate",
                resource_kind: "turn_candidate",
                resource_id: &run_id,
                now,
            },
            &result,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

struct LoadedTurn {
    view: ConversationTurnView,
    context_id: String,
    user_branch_id: String,
    content_id: String,
}

async fn load_turn<C: ConnectionTrait>(connection: &C, turn_id: &str) -> StorageResult<LoadedTurn> {
    let row = connection.query_one_raw(sql(
        "SELECT t.id, t.conversation_id, t.user_message_id, t.user_commit_id, t.created_at, c.context_id, vc.lineage_key AS user_branch_id, m.content_object_id FROM conversation_turns t JOIN conversations c ON c.id = t.conversation_id JOIN version_commits vc ON vc.id = t.user_commit_id AND vc.aggregate_kind = 'working_context' JOIN conversation_messages m ON m.id = t.user_message_id AND m.commit_id = t.user_commit_id WHERE t.id = ?",
        vec![turn_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "conversation_turn", id: turn_id.into() })?;
    Ok(LoadedTurn {
        view: ConversationTurnView {
            id: row.try_get("", "id")?,
            conversation_id: row.try_get("", "conversation_id")?,
            user_message_id: row.try_get("", "user_message_id")?,
            user_commit_id: row.try_get("", "user_commit_id")?,
            created_at: row.try_get("", "created_at")?,
        },
        context_id: row.try_get("", "context_id")?,
        user_branch_id: row.try_get("", "user_branch_id")?,
        content_id: row.try_get("", "content_object_id")?,
    })
}

fn validate(command: &RegenerateConversationCandidateCommand) -> StorageResult<()> {
    if command.turn_id.is_empty()
        || command.expected_user_commit_id.is_empty()
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid regenerate candidate command".into(),
        ));
    }
    Ok(())
}
