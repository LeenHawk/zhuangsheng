use sea_orm::TransactionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::{SubmitConversationTurnCommand, SubmitConversationTurnResult},
    conversation::{
        ConversationContextV1, ConversationRunInputV1, ConversationTurnView, TurnCandidateStatus,
        TurnCandidateView,
    },
    runtime::{RunContextCommand, StartRunCommand},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    context::{commit::commit_patch, query::load_context},
    graph::helpers::{new_id, put_inline_object},
    runtime::{query::load_run, start_insert::insert_new_run},
};

use super::{
    contract::validate_run_spec,
    events::append_event,
    read::load_conversation,
    receipt::{Receipt, finish, replay},
    submit_prepare::{prepare, user_message_patch},
    submit_rows::{advance_conversation, fork_candidate, insert_candidate, insert_message_turn},
    text_transform::canonical_user_content,
};

impl SqliteStore {
    pub async fn submit_conversation_turn_at(
        &self,
        command: SubmitConversationTurnCommand,
        now: i64,
    ) -> StorageResult<SubmitConversationTurnResult> {
        let prepared = prepare(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(result) = replay::<_, SubmitConversationTurnResult>(
            &transaction,
            &prepared.scope,
            &command.idempotency_key,
            &prepared.digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(result);
        }
        let conversation = load_conversation(&transaction, &command.conversation_id).await?;
        if conversation.active_head_commit_id != command.expected_head_commit_id {
            return Err(StorageError::Conflict("conversation_head"));
        }
        let definition = validate_run_spec(&transaction, &command.run).await?;
        let user_content =
            canonical_user_content(&transaction, &definition, &command.user_content).await?;
        let active = load_context(
            &transaction,
            &conversation.context_id,
            &conversation.active_branch_id,
        )
        .await?;
        let context: ConversationContextV1 = serde_json::from_value(active.value)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
        context
            .validate()
            .map_err(|message| StorageError::Integrity(message.into()))?;
        let turn_id = new_id("turn");
        let message_id = new_id("message");
        let run_id = new_id("run");
        let candidate_branch_id = new_id("branch");
        let content_id = put_inline_object(
            &transaction,
            &zhuangsheng_core::canonical::to_vec(&user_content)?,
            now,
        )
        .await?;
        let (message, patch) = user_message_patch(
            &conversation,
            &command,
            &context,
            &turn_id,
            &message_id,
            &content_id,
        )?;
        let commit = commit_patch(&transaction, &patch, now).await?;
        advance_conversation(
            &transaction,
            &conversation.id,
            &command.expected_head_commit_id,
            &commit.id,
            now,
        )
        .await?;
        insert_message_turn(
            &transaction,
            &conversation.id,
            &conversation.active_branch_id,
            &message,
            &commit.id,
            &content_id,
            &prepared.scope,
            &command.idempotency_key,
            now,
        )
        .await?;
        fork_candidate(
            &transaction,
            &conversation.context_id,
            &conversation.active_branch_id,
            &candidate_branch_id,
            &commit.id,
            &run_id,
            now,
        )
        .await?;
        let run_input = ConversationRunInputV1 {
            schema_version: 1,
            conversation_id: conversation.id.clone(),
            turn_id: turn_id.clone(),
            user_message_id: message_id,
            user_commit_id: commit.id.clone(),
            content: user_content,
        };
        insert_new_run(
            &transaction,
            &StartRunCommand {
                graph_revision_id: command.run.graph_revision_id,
                input: serde_json::to_value(run_input)
                    .map_err(|error| StorageError::Integrity(error.to_string()))?,
                context: RunContextCommand::Existing {
                    context_id: conversation.context_id.clone(),
                    branch_id: candidate_branch_id.clone(),
                    expected_head_commit_id: commit.id.clone(),
                },
                deadline_at: None,
                idempotency_key: format!("conversation-candidate:{run_id}"),
            },
            &run_id,
            now,
        )
        .await?;
        let turn = ConversationTurnView {
            id: turn_id.clone(),
            conversation_id: conversation.id.clone(),
            user_message_id: message.message_id,
            user_commit_id: commit.id.clone(),
            created_at: now,
        };
        let candidate = TurnCandidateView {
            turn_id,
            run_id: run_id.clone(),
            branch_id: candidate_branch_id,
            base_commit_id: commit.id,
            reply_output_key: command.run.reply_output_key,
            status: TurnCandidateStatus::Running,
            created_at: now,
        };
        insert_candidate(
            &transaction,
            &turn,
            &candidate,
            &prepared.scope,
            &command.idempotency_key,
        )
        .await?;
        let result = SubmitConversationTurnResult {
            turn,
            candidate,
            run: load_run(&transaction, &run_id).await?,
        };
        append_event(
            &transaction,
            &conversation.id,
            "conversation.turn_submitted",
            &json!({"schemaVersion":1,"turnId":result.turn.id,"runId":run_id}),
            now,
        )
        .await?;
        finish(
            &transaction,
            Receipt {
                scope: &prepared.scope,
                key: &command.idempotency_key,
                digest: &prepared.digest,
                command_kind: "conversation.turn.submit",
                resource_kind: "conversation_turn",
                resource_id: &result.turn.id,
                now,
            },
            &result,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}
