use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::{
    ConversationContextMessageV1, ConversationTurnView, TurnCandidateView,
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) async fn advance_conversation<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    old_head: &str,
    new_head: &str,
    now: i64,
) -> StorageResult<()> {
    let updated = connection.execute_raw(sql(
        "UPDATE conversations SET active_head_commit_id = ?, updated_at = ? WHERE id = ? AND active_head_commit_id = ?",
        vec![new_head.into(), now.into(), conversation_id.into(), old_head.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("conversation_head"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_message_turn<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    branch_id: &str,
    message: &ConversationContextMessageV1,
    commit_id: &str,
    content_id: &str,
    scope: &str,
    key: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO conversation_messages (id, conversation_id, turn_id, branch_id, commit_id, parent_message_id, role, source_kind, content_object_id, created_at) VALUES (?, ?, ?, ?, ?, ?, 'user', 'user_input', ?, ?)",
        vec![message.message_id.clone().into(), conversation_id.into(), message.turn_id.clone().into(), branch_id.into(), commit_id.into(), message.parent_message_id.clone().into(), content_id.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO conversation_turns (id, conversation_id, user_message_id, user_commit_id, request_scope, request_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![message.turn_id.clone().into(), conversation_id.into(), message.message_id.clone().into(), commit_id.into(), scope.into(), key.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'conversation_message', ?, 'content', ?)",
        vec![content_id.into(), message.message_id.clone().into(), now.into()],
    )).await?;
    Ok(())
}

pub(super) async fn fork_candidate<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    parent_branch_id: &str,
    branch_id: &str,
    commit_id: &str,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO context_branches (id, context_id, parent_branch_id, fork_commit_id, head_commit_id, creation_operation_id, status, pinned, audit_hold, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'active', 0, 0, ?, ?)",
        vec![branch_id.into(), context_id.into(), parent_branch_id.into(), commit_id.into(), commit_id.into(), format!("conversation-candidate-branch:{run_id}").into(), now.into(), now.into()],
    )).await?;
    let projection = connection.execute_raw(sql(
        "INSERT INTO materialized_projections (aggregate_kind, aggregate_id, lineage_key, head_commit_id, projection_json, projection_object_id, schema_version, updated_at) SELECT 'working_context', aggregate_id, ?, head_commit_id, projection_json, projection_object_id, schema_version, ? FROM materialized_projections WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND head_commit_id = ?",
        vec![branch_id.into(), now.into(), context_id.into(), parent_branch_id.into(), commit_id.into()],
    )).await?;
    if projection.rows_affected() != 1 {
        return Err(StorageError::Conflict("candidate_projection_base"));
    }
    Ok(())
}

pub(super) async fn insert_candidate<C: ConnectionTrait>(
    connection: &C,
    turn: &ConversationTurnView,
    candidate: &TurnCandidateView,
    scope: &str,
    key: &str,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO turn_candidates (turn_id, run_id, branch_id, base_commit_id, reply_output_key, creation_scope, creation_key, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'running', ?)",
        vec![turn.id.clone().into(), candidate.run_id.clone().into(), candidate.branch_id.clone().into(), candidate.base_commit_id.clone().into(), candidate.reply_output_key.clone().into(), scope.into(), key.into(), candidate.created_at.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO conversation_run_bindings (run_id, conversation_id, turn_id, reply_output_key) VALUES (?, ?, ?, ?)",
        vec![candidate.run_id.clone().into(), turn.conversation_id.clone().into(), turn.id.clone().into(), candidate.reply_output_key.clone().into()],
    )).await?;
    Ok(())
}
