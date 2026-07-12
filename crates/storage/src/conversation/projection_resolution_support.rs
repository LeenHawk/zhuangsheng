use sea_orm::ConnectionTrait;
use zhuangsheng_core::application::conversation::ResolveCandidateProjectionCommand;

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::projection::Candidate;

pub(super) struct ConflictedCandidate {
    pub candidate: Candidate,
    pub branch_head: String,
}

pub(super) async fn load_conflicted<C: ConnectionTrait>(
    connection: &C,
    turn_id: &str,
    run_id: &str,
) -> StorageResult<ConflictedCandidate> {
    let row = connection.query_one_raw(sql(
        "SELECT tc.status AS candidate_status, tc.branch_id, tc.base_commit_id, tc.reply_output_key, tc.assistant_message_id, tc.candidate_commit_id, tc.projection_error_object_id, t.user_message_id, t.user_commit_id, t.conversation_id, c.context_id, r.status AS run_status, r.context_id AS run_context, r.branch_id AS run_branch, r.output_commit_id, j.status AS job_status, j.terminal_status, b.context_id AS branch_context, b.status AS branch_status, b.head_commit_id AS branch_head, oc.aggregate_id AS output_context, pe.lifecycle AS error_lifecycle, EXISTS (SELECT 1 FROM content_object_refs ref WHERE ref.object_id = tc.projection_error_object_id AND ref.owner_kind = 'turn_candidate' AND ref.owner_id = tc.run_id AND ref.role = 'projection_error') AS error_ref FROM turn_candidates tc JOIN conversation_turns t ON t.id = tc.turn_id JOIN conversations c ON c.id = t.conversation_id JOIN graph_runs r ON r.id = tc.run_id JOIN candidate_projection_jobs j ON j.run_id = tc.run_id JOIN context_branches b ON b.id = tc.branch_id LEFT JOIN version_commits oc ON oc.id = r.output_commit_id AND oc.aggregate_kind = 'working_context' LEFT JOIN content_objects pe ON pe.id = tc.projection_error_object_id WHERE tc.turn_id = ? AND tc.run_id = ?",
        vec![turn_id.into(), run_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound {
        kind: "turn_candidate",
        id: format!("{turn_id}:{run_id}"),
    })?;
    if row.try_get::<String>("", "candidate_status")? != "projection_conflicted" {
        return Err(StorageError::Conflict("candidate_projection_status"));
    }
    let context_id: String = row.try_get("", "context_id")?;
    let branch_id: String = row.try_get("", "branch_id")?;
    let output_commit_id: Option<String> = row.try_get("", "output_commit_id")?;
    let projection_error_id: Option<String> = row.try_get("", "projection_error_object_id")?;
    if row.try_get::<String>("", "job_status")? != "conflicted"
        || row.try_get::<String>("", "terminal_status")? != "completed"
        || row.try_get::<String>("", "run_status")? != "completed"
        || row.try_get::<String>("", "run_context")? != context_id
        || row.try_get::<String>("", "run_branch")? != branch_id
        || row.try_get::<String>("", "branch_context")? != context_id
        || row.try_get::<String>("", "branch_status")? != "active"
        || row.try_get::<String>("", "base_commit_id")?
            != row.try_get::<String>("", "user_commit_id")?
        || output_commit_id.is_none()
        || row
            .try_get::<Option<String>>("", "output_context")?
            .as_deref()
            != Some(context_id.as_str())
        || projection_error_id.is_none()
        || row
            .try_get::<Option<String>>("", "error_lifecycle")?
            .as_deref()
            != Some("live")
        || row.try_get::<i64>("", "error_ref")? != 1
        || row
            .try_get::<Option<String>>("", "assistant_message_id")?
            .is_some()
        || row
            .try_get::<Option<String>>("", "candidate_commit_id")?
            .is_some()
    {
        return Err(StorageError::Integrity(
            "conflicted candidate projection is corrupt".into(),
        ));
    }
    Ok(ConflictedCandidate {
        branch_head: row.try_get("", "branch_head")?,
        candidate: Candidate {
            terminal_status: "completed".into(),
            turn_id: turn_id.into(),
            branch_id,
            base_commit_id: row.try_get("", "base_commit_id")?,
            reply_output_key: row.try_get("", "reply_output_key")?,
            user_message_id: row.try_get("", "user_message_id")?,
            conversation_id: row.try_get("", "conversation_id")?,
            context_id,
            output_commit_id,
        },
    })
}

pub(super) fn validate(command: &ResolveCandidateProjectionCommand) -> StorageResult<()> {
    let reason = command.resolution.reason();
    if command.turn_id.is_empty()
        || command.turn_id.len() > 128
        || command.run_id.is_empty()
        || command.run_id.len() > 128
        || command.expected_current_branch_head.is_empty()
        || command.expected_current_branch_head.len() > 128
        || reason.trim().is_empty()
        || reason.len() > 1_000
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid candidate projection resolution".into(),
        ));
    }
    Ok(())
}
