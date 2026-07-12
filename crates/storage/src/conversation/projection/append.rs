use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    canonical,
    conversation::{
        AssistantReplyPayloadV1, ConversationContextMessageV1, ConversationMessageRole,
        ConversationMessageSource, assistant_reply_payload_v1_schema,
    },
    schema,
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{
    StorageError, StorageResult,
    context::commit::commit_patch,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

use super::{
    complete::Candidate,
    outcome::{finish_job, permanent_failure, projection_conflict},
};
use crate::conversation::events::append_event;
use crate::conversation::selection::auto_select;

pub(super) async fn project_completed<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    candidate: Candidate,
    now: i64,
) -> StorageResult<()> {
    let Some(output_commit_id) = candidate.output_commit_id.clone() else {
        return permanent_failure(connection, run_id, "missing output commit", now).await;
    };
    let head = connection.query_one_raw(sql(
        "SELECT head_commit_id FROM context_branches WHERE context_id = ? AND id = ? AND status = 'active'",
        vec![candidate.context_id.clone().into(), candidate.branch_id.clone().into()],
    )).await?;
    let Some(head) = head else {
        return permanent_failure(connection, run_id, "candidate branch is unavailable", now).await;
    };
    if head.try_get::<String>("", "head_commit_id")? != output_commit_id {
        return projection_conflict(connection, run_id, "candidate branch head changed", now).await;
    }
    let rows = connection.query_all_raw(sql(
        "SELECT value_object_id FROM run_output_values WHERE run_id = ? AND output_key = ? ORDER BY output_seq",
        vec![run_id.into(), candidate.reply_output_key.clone().into()],
    )).await?;
    if rows.len() != 1 {
        return permanent_failure(
            connection,
            run_id,
            "reply output cardinality is invalid",
            now,
        )
        .await;
    }
    let value_id: String = rows[0].try_get("", "value_object_id")?;
    let value: serde_json::Value = match load_object_json(connection, &value_id).await {
        Ok(value) => value,
        Err(StorageError::Integrity(_)) => {
            return permanent_failure(connection, run_id, "reply output is corrupt", now).await;
        }
        Err(error) => return Err(error),
    };
    if schema::validate(&assistant_reply_payload_v1_schema(), &value).is_err() {
        return permanent_failure(connection, run_id, "reply output schema is invalid", now).await;
    }
    let payload: AssistantReplyPayloadV1 = match serde_json::from_value(value) {
        Ok(payload) => payload,
        Err(_) => {
            return permanent_failure(connection, run_id, "reply output cannot be decoded", now)
                .await;
        }
    };
    if payload.validate().is_err() {
        return permanent_failure(connection, run_id, "reply content is invalid", now).await;
    }
    append_assistant(
        connection,
        run_id,
        candidate,
        output_commit_id,
        payload,
        now,
    )
    .await
}

async fn append_assistant<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    candidate: Candidate,
    output_commit_id: String,
    payload: AssistantReplyPayloadV1,
    now: i64,
) -> StorageResult<()> {
    let message_id = new_id("message");
    let content_id =
        put_inline_object(connection, &canonical::to_vec(&payload.content)?, now).await?;
    let message = ConversationContextMessageV1 {
        message_id: message_id.clone(),
        turn_id: candidate.turn_id.clone(),
        role: ConversationMessageRole::Assistant,
        source: ConversationMessageSource::RunOutput,
        content_ref: content_id.clone(),
        parent_message_id: Some(candidate.user_message_id.clone()),
        origin_run_id: Some(run_id.into()),
    };
    let value = serde_json::to_value(&message)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let commit = commit_patch(
        connection,
        &CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: candidate.context_id.clone(),
                lineage_key: candidate.branch_id.clone(),
                base_commit_id: output_commit_id,
                operation_id: format!("conversation-assistant-message:{message_id}"),
                ops: vec![JsonPatchOp::Append {
                    path: "/messages".into(),
                    element_id: message_id.clone(),
                    value,
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::Application,
                    id: None,
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        },
        now,
    )
    .await?;
    insert_ready_rows(
        connection,
        run_id,
        &candidate,
        &message,
        &commit.id,
        &content_id,
        now,
    )
    .await?;
    append_event(
        connection,
        &candidate.conversation_id,
        "conversation.candidate_ready",
        &json!({"schemaVersion":1,"turnId":candidate.turn_id,"runId":run_id,"messageId":message_id,"commitId":commit.id}),
        now,
    )
    .await?;
    auto_select(
        connection,
        &candidate.conversation_id,
        &candidate.turn_id,
        run_id,
        &candidate.branch_id,
        &candidate.base_commit_id,
        &commit.id,
        now,
    )
    .await?;
    finish_job(connection, run_id, "done", None, now).await
}

#[allow(clippy::too_many_arguments)]
async fn insert_ready_rows<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    candidate: &Candidate,
    message: &ConversationContextMessageV1,
    commit_id: &str,
    content_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO conversation_messages (id, conversation_id, turn_id, branch_id, commit_id, parent_message_id, role, source_kind, content_object_id, origin_run_id, created_at) VALUES (?, ?, ?, ?, ?, ?, 'assistant', 'run_output', ?, ?, ?)",
        vec![message.message_id.clone().into(), candidate.conversation_id.clone().into(), candidate.turn_id.clone().into(), candidate.branch_id.clone().into(), commit_id.into(), message.parent_message_id.clone().into(), content_id.into(), run_id.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'conversation_message', ?, 'content', ?)",
        vec![content_id.into(), message.message_id.clone().into(), now.into()],
    )).await?;
    let updated = connection.execute_raw(sql(
        "UPDATE turn_candidates SET status = 'ready', assistant_message_id = ?, candidate_commit_id = ? WHERE run_id = ? AND status = 'running'",
        vec![message.message_id.clone().into(), commit_id.into(), run_id.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("candidate_status"));
    }
    Ok(())
}
