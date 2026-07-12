use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::{
        CandidateProjectionResolution, ResolveCandidateProjectionCommand,
        ResolveCandidateProjectionResult,
    },
    canonical,
    conversation::TurnCandidateStatus,
};

use crate::{SqliteStore, StorageError, StorageResult, context::is_reachable, graph::helpers::sql};

use super::{
    events::append_event,
    projection::{ReplyPayloadError, append_ready_candidate, load_reply_payload},
    projection_resolution_support::{ConflictedCandidate, load_conflicted, validate},
    receipt::{Receipt, finish, replay},
};

impl SqliteStore {
    pub async fn resolve_candidate_projection_at(
        &self,
        command: ResolveCandidateProjectionCommand,
        now: i64,
    ) -> StorageResult<ResolveCandidateProjectionResult> {
        validate(&command)?;
        let scope = format!(
            "conversation:candidate-projection:{}:{}",
            command.turn_id, command.run_id
        );
        let digest = canonical::hash(&json!({
            "schemaVersion":1,"command":"resolve_candidate_projection",
            "turnId":command.turn_id,"runId":command.run_id,
            "expectedCurrentBranchHead":command.expected_current_branch_head,
            "resolution":command.resolution,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(result) = replay::<_, ResolveCandidateProjectionResult>(
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
        let conflicted = load_conflicted(&transaction, &command.turn_id, &command.run_id).await?;
        if conflicted.branch_head != command.expected_current_branch_head {
            return Err(StorageError::Conflict("candidate_branch_head"));
        }
        let conversation_id = conflicted.candidate.conversation_id.clone();
        let reason = command.resolution.reason().to_owned();
        let mode = match &command.resolution {
            CandidateProjectionResolution::AppendAfterCurrent { .. } => "append_after_current",
            CandidateProjectionResolution::AbandonProjection { .. } => "abandon_projection",
        };
        let result = if mode == "append_after_current" {
            append_after_current(&transaction, &command, conflicted, now).await?
        } else {
            abandon(&transaction, &command, &conflicted, now).await?
        };
        append_event(
            &transaction,
            &conversation_id,
            "conversation.candidate_projection_resolved",
            &json!({
                "schemaVersion":1,"turnId":command.turn_id,"runId":command.run_id,
                "mode":mode,"reason":reason,
                "previousHeadCommitId":command.expected_current_branch_head,
                "resultHeadCommitId":result.branch_head_commit_id,
            }),
            now,
        )
        .await?;
        finish(
            &transaction,
            Receipt {
                scope: &scope,
                key: &command.idempotency_key,
                digest: &digest,
                command_kind: "conversation.candidate_projection.resolve",
                resource_kind: "turn_candidate",
                resource_id: &command.run_id,
                now,
            },
            &result,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

async fn append_after_current<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveCandidateProjectionCommand,
    conflicted: ConflictedCandidate,
    now: i64,
) -> StorageResult<ResolveCandidateProjectionResult> {
    let output_commit = conflicted
        .candidate
        .output_commit_id
        .as_deref()
        .ok_or_else(|| {
            StorageError::Integrity("completed candidate output commit is missing".into())
        })?;
    if !is_reachable(
        connection,
        &command.expected_current_branch_head,
        output_commit,
    )
    .await?
    {
        return Err(StorageError::Conflict("candidate_output_not_ancestor"));
    }
    let payload = match load_reply_payload(
        connection,
        &command.run_id,
        &conflicted.candidate.reply_output_key,
    )
    .await
    {
        Ok(payload) => payload,
        Err(ReplyPayloadError::Invalid(message)) => {
            return Err(StorageError::InvalidArgument(message.into()));
        }
        Err(ReplyPayloadError::Storage(error)) => return Err(error),
    };
    let branch_id = conflicted.candidate.branch_id.clone();
    let ready = append_ready_candidate(
        connection,
        &command.run_id,
        conflicted.candidate,
        command.expected_current_branch_head.clone(),
        payload,
        "projection_conflicted",
        false,
        now,
    )
    .await?;
    Ok(ResolveCandidateProjectionResult {
        turn_id: command.turn_id.clone(),
        run_id: command.run_id.clone(),
        branch_id,
        branch_head_commit_id: ready.commit_id.clone(),
        status: TurnCandidateStatus::Ready,
        assistant_message_id: Some(ready.message_id),
        candidate_commit_id: Some(ready.commit_id),
        resolved_at: now,
    })
}

async fn abandon<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveCandidateProjectionCommand,
    conflicted: &ConflictedCandidate,
    now: i64,
) -> StorageResult<ResolveCandidateProjectionResult> {
    let updated = connection.execute_raw(sql(
        "UPDATE turn_candidates SET status = 'projection_abandoned' WHERE turn_id = ? AND run_id = ? AND status = 'projection_conflicted' AND EXISTS (SELECT 1 FROM context_branches WHERE id = turn_candidates.branch_id AND head_commit_id = ? AND status = 'active')",
        vec![command.turn_id.clone().into(), command.run_id.clone().into(), command.expected_current_branch_head.clone().into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("candidate_branch_head"));
    }
    Ok(ResolveCandidateProjectionResult {
        turn_id: command.turn_id.clone(),
        run_id: command.run_id.clone(),
        branch_id: conflicted.candidate.branch_id.clone(),
        branch_head_commit_id: command.expected_current_branch_head.clone(),
        status: TurnCandidateStatus::ProjectionAbandoned,
        assistant_message_id: None,
        candidate_commit_id: None,
        resolved_at: now,
    })
}
