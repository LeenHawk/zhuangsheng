use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    llm::{FinishToolCallCommand, ToolCallCheckpointStatus, ToolCallOutcome},
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::put_inline_object};

use super::{
    effect_wait::{
        EffectWait, EffectWaitOwner, allocate_wait_id, load_wait_ids, open_effect_resolution_wait,
    },
    model_ledger_helpers::{add_ref, persist_checkpoint},
    tool_ledger_helpers::append_tool_event,
    tool_validation::{
        FencedToolCall, ToolCheckpointExpectation, load_tool_attempt, validate_tool_checkpoint,
        validate_tool_fence, validate_tool_replay_fence,
    },
    validation::load_ledger_context,
};

impl SqliteStore {
    pub async fn finish_tool_call(
        &self,
        command: FinishToolCallCommand,
        now: i64,
    ) -> StorageResult<zhuangsheng_core::llm::LlmLoopCheckpoint> {
        let transaction = self.db.begin().await?;
        let call =
            load_tool_attempt(&transaction, &command.effect_attempt_id, &command.fence).await?;
        let context = load_ledger_context(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        let fingerprint = outcome_fingerprint(&command.outcome)?;
        let mut checkpoint = command.checkpoint;
        let durable_wait_ids = load_wait_ids(&transaction, &call.node_instance_id).await?;
        if durable_wait_ids.is_empty() {
            if !checkpoint.wait_ids.is_empty() {
                return Err(StorageError::InvalidArgument(
                    "tool finish checkpoint contains unknown wait ids".into(),
                ));
            }
        } else {
            checkpoint.wait_ids = durable_wait_ids;
        }
        if let Some(active) = checkpoint
            .current_batch
            .iter_mut()
            .find(|item| item.tool_call_id == call.tool_call_id)
        {
            active.output_ref = call.output_object_id.clone();
        }
        checkpoint = checkpoint.seal()?;
        if terminal_matches(&call, &fingerprint) {
            validate_tool_replay_fence(&call, &command.fence)?;
            if !payload_matches(&call, &fingerprint)
                || call.checkpoint_digest.as_deref() != Some(&checkpoint.checksum)
            {
                return Err(StorageError::Conflict("tool_call_finish_replay"));
            }
            validate_tool_checkpoint(
                &checkpoint,
                expectation(
                    &context,
                    &call,
                    &command.effect_attempt_id,
                    &command.fence.invoking_node_attempt_id,
                    fingerprint.checkpoint_status,
                    call.output_object_id.as_deref(),
                ),
            )?;
            transaction.commit().await?;
            return Ok(checkpoint);
        }
        validate_tool_fence(&call, &command.fence)?;
        if call.attempt_status != "started"
            || call.effect_status != "pending"
            || call.tool_status != "running"
        {
            return Err(StorageError::Conflict("tool_effect_status"));
        }
        if matches!(command.outcome, ToolCallOutcome::RetryReady { .. })
            && call.classification == "non_idempotent"
        {
            return Err(StorageError::InvalidArgument(
                "non-idempotent started tool effect cannot become retry-ready".into(),
            ));
        }
        if matches!(command.outcome, ToolCallOutcome::OutcomeUnknown { .. })
            && call.classification != "non_idempotent"
        {
            return Err(StorageError::InvalidArgument(
                "retry-safe tool effects must use retry-ready instead of human coordination".into(),
            ));
        }
        let stored = store_outcome(&transaction, &command.outcome, now).await?;
        let wait_id = if stored.tool_status == "outcome_unknown" {
            let wait_id = allocate_wait_id(&transaction, &call.node_instance_id).await?;
            checkpoint.wait_ids.push(wait_id.clone());
            Some(wait_id)
        } else {
            None
        };
        if let Some(active) = checkpoint
            .current_batch
            .iter_mut()
            .find(|item| item.tool_call_id == call.tool_call_id)
        {
            active.output_ref = stored.output_object_id.clone();
        }
        checkpoint = checkpoint.seal()?;
        validate_tool_checkpoint(
            &checkpoint,
            expectation(
                &context,
                &call,
                &command.effect_attempt_id,
                &command.fence.invoking_node_attempt_id,
                stored.checkpoint_status,
                stored.output_object_id.as_deref(),
            ),
        )?;
        finish_rows(
            &transaction,
            &call,
            &command.effect_attempt_id,
            &stored,
            now,
        )
        .await?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        if let Some(wait_id) = &wait_id {
            open_effect_resolution_wait(
                &transaction,
                EffectWait {
                    wait_id,
                    node_instance_id: &call.node_instance_id,
                    invoking_node_attempt_id: &command.fence.invoking_node_attempt_id,
                    owner: EffectWaitOwner::Tool {
                        tool_call_id: &call.tool_call_id,
                    },
                    effect_id: &call.effect_id,
                    effect_attempt_id: &command.effect_attempt_id,
                    classification: &call.classification,
                },
                now,
            )
            .await?;
        }
        add_outcome_refs(
            &transaction,
            &call,
            &command.effect_attempt_id,
            &stored,
            now,
        )
        .await?;
        append_tool_event(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
            match stored.tool_status {
                "completed" => "llm.tool.completed",
                "failed" => "llm.tool.failed",
                "outcome_unknown" => "llm.tool.outcome_unknown",
                _ => "llm.tool.retry_ready",
            },
            json!({
                "schemaVersion":1,
                "toolCallId":call.tool_call_id,
                "effectId":call.effect_id,
                "effectAttemptId":command.effect_attempt_id,
                "callIndex":call.call_index,
                "outputRef":stored.output_object_id,
                "waitId":wait_id,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(checkpoint)
    }
}

struct OutcomeFingerprint {
    tool_status: &'static str,
    effect_status: &'static str,
    attempt_status: &'static str,
    checkpoint_status: ToolCallCheckpointStatus,
    content_digest: String,
    has_output: bool,
}

struct StoredToolOutcome {
    tool_status: &'static str,
    effect_status: &'static str,
    attempt_status: &'static str,
    checkpoint_status: ToolCallCheckpointStatus,
    output_object_id: Option<String>,
    error_object_id: Option<String>,
    effect_completed: bool,
}

fn outcome_fingerprint(outcome: &ToolCallOutcome) -> StorageResult<OutcomeFingerprint> {
    Ok(match outcome {
        ToolCallOutcome::Completed { output_bytes } => {
            validate_tool_output(output_bytes)?;
            OutcomeFingerprint {
                tool_status: "completed",
                effect_status: "succeeded",
                attempt_status: "succeeded",
                checkpoint_status: ToolCallCheckpointStatus::Completed,
                content_digest: canonical::hash_bytes(output_bytes),
                has_output: true,
            }
        }
        ToolCallOutcome::Failed { error_bytes } => fingerprint_error(
            "failed",
            "failed",
            "failed",
            ToolCallCheckpointStatus::Failed,
            error_bytes,
        ),
        ToolCallOutcome::OutcomeUnknown { error_bytes } => fingerprint_error(
            "outcome_unknown",
            "outcome_unknown",
            "outcome_unknown",
            ToolCallCheckpointStatus::OutcomeUnknown,
            error_bytes,
        ),
        ToolCallOutcome::RetryReady { error_bytes } => fingerprint_error(
            "retry_ready",
            "pending",
            "outcome_unknown",
            ToolCallCheckpointStatus::RetryReady,
            error_bytes,
        ),
    })
}

fn fingerprint_error(
    tool_status: &'static str,
    effect_status: &'static str,
    attempt_status: &'static str,
    checkpoint_status: ToolCallCheckpointStatus,
    bytes: &[u8],
) -> OutcomeFingerprint {
    OutcomeFingerprint {
        tool_status,
        effect_status,
        attempt_status,
        checkpoint_status,
        content_digest: canonical::hash_bytes(bytes),
        has_output: false,
    }
}

async fn store_outcome<C: ConnectionTrait>(
    connection: &C,
    outcome: &ToolCallOutcome,
    now: i64,
) -> StorageResult<StoredToolOutcome> {
    Ok(match outcome {
        ToolCallOutcome::Completed { output_bytes } => StoredToolOutcome {
            tool_status: "completed",
            effect_status: "succeeded",
            attempt_status: "succeeded",
            checkpoint_status: ToolCallCheckpointStatus::Completed,
            output_object_id: Some(put_inline_object(connection, output_bytes, now).await?),
            error_object_id: None,
            effect_completed: true,
        },
        ToolCallOutcome::Failed { error_bytes } => StoredToolOutcome {
            tool_status: "failed",
            effect_status: "failed",
            attempt_status: "failed",
            checkpoint_status: ToolCallCheckpointStatus::Failed,
            output_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            effect_completed: true,
        },
        ToolCallOutcome::OutcomeUnknown { error_bytes } => StoredToolOutcome {
            tool_status: "outcome_unknown",
            effect_status: "outcome_unknown",
            attempt_status: "outcome_unknown",
            checkpoint_status: ToolCallCheckpointStatus::OutcomeUnknown,
            output_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            effect_completed: true,
        },
        ToolCallOutcome::RetryReady { error_bytes } => StoredToolOutcome {
            tool_status: "retry_ready",
            effect_status: "pending",
            attempt_status: "outcome_unknown",
            checkpoint_status: ToolCallCheckpointStatus::RetryReady,
            output_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            effect_completed: false,
        },
    })
}

pub(super) fn validate_tool_output(bytes: &[u8]) -> StorageResult<()> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(StorageError::InvalidArgument(
            "tool output exceeds supported bounds".into(),
        ));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| StorageError::InvalidArgument("tool output is not valid JSON".into()))?;
    if canonical::to_vec(&value)? != bytes {
        return Err(StorageError::InvalidArgument(
            "tool output must use canonical JSON encoding".into(),
        ));
    }
    let parts = value
        .get("parts")
        .and_then(Value::as_array)
        .filter(|parts| !parts.is_empty() && parts.len() <= 64)
        .ok_or_else(|| StorageError::InvalidArgument("tool output parts are invalid".into()))?;
    let mut llm_results = 0;
    for part in parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or_default();
        if !matches!(
            kind,
            "llm_result"
                | "artifact"
                | "state_patch"
                | "memory_change_proposal"
                | "user_message"
                | "evidence"
                | "debug"
        ) {
            return Err(StorageError::InvalidArgument(
                "tool output contains an unknown part".into(),
            ));
        }
        if kind == "llm_result" {
            llm_results += 1;
            if part
                .get("content")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
            {
                return Err(StorageError::InvalidArgument(
                    "tool llm_result content is empty".into(),
                ));
            }
        }
    }
    if llm_results != 1 {
        return Err(StorageError::InvalidArgument(
            "successful tool output requires exactly one llm_result".into(),
        ));
    }
    Ok(())
}

fn terminal_matches(call: &FencedToolCall, expected: &OutcomeFingerprint) -> bool {
    call.tool_status == expected.tool_status
        && call.effect_status == expected.effect_status
        && call.attempt_status == expected.attempt_status
}

fn payload_matches(call: &FencedToolCall, expected: &OutcomeFingerprint) -> bool {
    if expected.has_output {
        call.output_digest.as_deref() == Some(&expected.content_digest)
            && call.error_digest.is_none()
    } else {
        call.output_object_id.is_none()
            && call.error_digest.as_deref() == Some(&expected.content_digest)
    }
}

fn expectation<'a>(
    context: &'a super::validation::LedgerContext,
    call: &'a FencedToolCall,
    effect_attempt_id: &'a str,
    updater_attempt_id: &'a str,
    status: ToolCallCheckpointStatus,
    output_ref: Option<&'a str>,
) -> ToolCheckpointExpectation<'a> {
    ToolCheckpointExpectation {
        context,
        node_instance_id: &call.node_instance_id,
        updater_attempt_id,
        model_call_id: &call.model_call_id,
        tool_call_id: &call.tool_call_id,
        effect_id: &call.effect_id,
        effect_attempt_id,
        call_index: call.call_index,
        call_digest: &call.call_digest,
        expected_tool_calls_used: call.tool_calls_used,
        status,
        output_ref,
    }
}

async fn finish_rows<C: ConnectionTrait>(
    connection: &C,
    call: &FencedToolCall,
    effect_attempt_id: &str,
    outcome: &StoredToolOutcome,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute_raw(crate::graph::helpers::sql(
        "UPDATE effect_attempts SET status = ?, result_object_id = ?, error_object_id = ?, finished_at = ? WHERE id = ? AND status = 'started'",
        vec![outcome.attempt_status.into(), outcome.output_object_id.clone().into(), outcome.error_object_id.clone().into(), now.into(), effect_attempt_id.into()],
    )).await?;
    let effect = connection.execute_raw(crate::graph::helpers::sql(
        "UPDATE effects SET status = ?, result_object_id = ?, completed_at = ? WHERE id = ? AND status = 'pending'",
        vec![outcome.effect_status.into(), outcome.output_object_id.clone().into(), outcome.effect_completed.then_some(now).into(), call.effect_id.clone().into()],
    )).await?;
    let tool = connection.execute_raw(crate::graph::helpers::sql(
        "UPDATE tool_calls SET status = ?, output_object_id = ?, error_object_id = ?, finished_at = ? WHERE id = ? AND status = 'running'",
        vec![outcome.tool_status.into(), outcome.output_object_id.clone().into(), outcome.error_object_id.clone().into(), now.into(), call.tool_call_id.clone().into()],
    )).await?;
    if attempt.rows_affected() != 1 || effect.rows_affected() != 1 || tool.rows_affected() != 1 {
        return Err(StorageError::Conflict("tool_effect_terminal_status"));
    }
    Ok(())
}

async fn add_outcome_refs<C: ConnectionTrait>(
    connection: &C,
    call: &FencedToolCall,
    effect_attempt_id: &str,
    outcome: &StoredToolOutcome,
    now: i64,
) -> StorageResult<()> {
    if let Some(object_id) = &outcome.output_object_id {
        for (kind, id) in [
            ("tool_call", call.tool_call_id.as_str()),
            ("effect", call.effect_id.as_str()),
            ("effect_attempt", effect_attempt_id),
        ] {
            add_ref(connection, object_id, kind, id, "result", now).await?;
        }
    }
    if let Some(object_id) = &outcome.error_object_id {
        add_ref(
            connection,
            object_id,
            "effect_attempt",
            effect_attempt_id,
            "error",
            now,
        )
        .await?;
        add_ref(
            connection,
            object_id,
            "tool_call",
            &call.tool_call_id,
            "error",
            now,
        )
        .await?;
    }
    Ok(())
}
