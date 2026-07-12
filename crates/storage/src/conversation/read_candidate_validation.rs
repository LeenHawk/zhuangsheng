use sea_orm::QueryResult;
use zhuangsheng_core::conversation::{ConversationTurnView, TurnCandidateStatus};

use crate::{StorageError, StorageResult};

pub(super) fn validate_candidate(
    row: &QueryResult,
    turn: &ConversationTurnView,
    status: TurnCandidateStatus,
    has_error: bool,
) -> StorageResult<()> {
    let context_id: String = row.try_get("", "context_id")?;
    let branch_id: String = row.try_get("", "branch_id")?;
    let assistant_id: Option<String> = row.try_get("", "assistant_message_id")?;
    let commit_id: Option<String> = row.try_get("", "candidate_commit_id")?;
    let ready = status == TurnCandidateStatus::Ready;
    let error_required = matches!(
        status,
        TurnCandidateStatus::ProjectionConflicted
            | TurnCandidateStatus::ProjectionFailed
            | TurnCandidateStatus::ProjectionAbandoned
    );
    let run_status: String = row.try_get("", "run_status")?;
    let run_matches = match status {
        TurnCandidateStatus::Running => true,
        TurnCandidateStatus::Ready
        | TurnCandidateStatus::ProjectionConflicted
        | TurnCandidateStatus::ProjectionFailed
        | TurnCandidateStatus::ProjectionAbandoned => run_status == "completed",
        TurnCandidateStatus::Failed => run_status == "failed",
        TurnCandidateStatus::Cancelled => run_status == "cancelled",
    };
    if row.try_get::<String>("", "base_commit_id")? != turn.user_commit_id
        || row.try_get::<String>("", "branch_context")? != context_id
        || row.try_get::<String>("", "run_context")? != context_id
        || row.try_get::<String>("", "run_branch")? != branch_id
        || !run_matches
        || ready != (assistant_id.is_some() && commit_id.is_some())
        || (!ready && (assistant_id.is_some() || commit_id.is_some()))
        || (error_required && !has_error)
        || (!error_required && !ready && has_error)
    {
        return Err(StorageError::Integrity(
            "conversation candidate row is inconsistent".into(),
        ));
    }
    if ready
        && (row.try_get::<Option<String>>("", "assistant_commit")? != commit_id
            || row
                .try_get::<Option<String>>("", "candidate_context")?
                .as_deref()
                != Some(context_id.as_str())
            || row
                .try_get::<Option<String>>("", "candidate_branch")?
                .as_deref()
                != Some(branch_id.as_str()))
    {
        return Err(StorageError::Integrity(
            "ready conversation candidate commit is inconsistent".into(),
        ));
    }
    Ok(())
}
