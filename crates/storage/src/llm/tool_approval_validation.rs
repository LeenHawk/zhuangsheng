use std::collections::BTreeSet;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::ApprovalRequiredAction,
    llm::{
        LlmLogicalCallStatus, PrepareToolApprovalBatchCommand, TOOL_CALL_POLICY_VERSION,
        ToolCallCheckpointStatus,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::{tool_validation::ValidatedToolCall, validation::LedgerContext};

pub(super) async fn validate_new_batch<C: ConnectionTrait>(
    connection: &C,
    context: &LedgerContext,
    command: &PrepareToolApprovalBatchCommand,
    validated: &[ValidatedToolCall],
    now: i64,
) -> StorageResult<()> {
    if !validated.iter().any(|call| call.requires_approval) {
        return Err(StorageError::InvalidArgument(
            "approval batch has no approval blockers".into(),
        ));
    }
    for (call, material) in command.calls.iter().zip(validated) {
        if material.requires_approval {
            if call.approval_expires_at <= now || call.risk_summary.is_empty() {
                return Err(StorageError::InvalidArgument(
                    "approval metadata is missing or expired".into(),
                ));
            }
            if material
                .grant
                .failure_policy
                .as_ref()
                .is_some_and(|policy| policy.approval_required == ApprovalRequiredAction::FailNode)
            {
                return Err(StorageError::InvalidArgument(
                    "tool policy requires node failure instead of approval".into(),
                ));
            }
        }
    }
    if connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM node_waits WHERE node_instance_id = ? AND status = 'open'",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .is_some()
    {
        return Err(StorageError::Conflict("node_instance_open_wait"));
    }
    let model = connection
        .query_one_raw(sql(
            "SELECT node_instance_id, status FROM model_calls WHERE id = ?",
            vec![command.model_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "model_call",
            id: command.model_call_id.clone(),
        })?;
    if model.try_get::<String>("", "node_instance_id")? != command.node_instance_id
        || model.try_get::<String>("", "status")? != "completed"
    {
        return Err(StorageError::InvalidArgument(
            "tool approval model owner is incompatible".into(),
        ));
    }
    let existing_model = count_calls(connection, "model_call_id", &command.model_call_id).await?;
    if existing_model != 0 || command.calls[0].call_index != 0 {
        return Err(StorageError::InvalidArgument(
            "approval batch must contain the complete model tool batch".into(),
        ));
    }
    let existing_total =
        count_calls(connection, "node_instance_id", &command.node_instance_id).await?;
    let expected_used = existing_total
        .checked_add(command.calls.len() as u64)
        .ok_or_else(|| StorageError::Integrity("tool-call count overflow".into()))?;
    let limit = context
        .snapshot
        .limits
        .max_tool_calls
        .ok_or_else(|| StorageError::Integrity("tool-call limit is not pinned".into()))?;
    if expected_used > limit {
        return Err(StorageError::InvalidArgument(
            "tool-call limit exceeded".into(),
        ));
    }
    validate_batch_checkpoint(connection, context, command, expected_used).await
}

async fn validate_batch_checkpoint<C: ConnectionTrait>(
    connection: &C,
    context: &LedgerContext,
    command: &PrepareToolApprovalBatchCommand,
    expected_used: u64,
) -> StorageResult<()> {
    let checkpoint = &command.checkpoint;
    let historical: Vec<String> = connection
        .query_all_raw(sql(
            "SELECT id FROM node_waits WHERE node_instance_id = ? ORDER BY created_at, id",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "id"))
        .collect::<Result<_, _>>()?;
    let mut expected_waits = historical;
    expected_waits.push(command.wait_id.clone());
    let active_model_matches = checkpoint
        .active_model_effect
        .as_ref()
        .is_some_and(|active| {
            active.model_call_id == command.model_call_id
                && active.status == LlmLogicalCallStatus::Completed
        });
    let calls_match = checkpoint.current_batch.len() == command.calls.len()
        && checkpoint
            .current_batch
            .iter()
            .zip(&command.calls)
            .all(|(stored, call)| {
                let expected_status = if context
                    .snapshot
                    .tools
                    .iter()
                    .find(|grant| grant.binding_id == call.binding_id)
                    .is_some_and(|grant| {
                        call.descriptor_requires_approval
                            || grant.approval
                                == Some(zhuangsheng_core::graph::ToolApprovalPolicy::Always)
                    }) {
                    ToolCallCheckpointStatus::AwaitingApproval
                } else {
                    ToolCallCheckpointStatus::Validated
                };
                stored.tool_call_id == call.tool_call_id
                    && stored.call_index == call.call_index
                    && stored.call_digest == call.call_digest
                    && stored.status == expected_status
                    && stored.effect_id.is_none()
                    && stored.output_ref.is_none()
                    && stored.wait_id.as_deref() == Some(command.wait_id.as_str())
            });
    if checkpoint.schema_version != 1
        || !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != command.node_instance_id
        || checkpoint.last_updated_by_attempt_id != command.originating_attempt_id
        || checkpoint.graph_revision_id != context.graph_revision_id
        || checkpoint.context_snapshot_ref != context.execution_snapshot_object_id
        || checkpoint.tool_calls_used != expected_used
        || checkpoint.effect_watermark != command.wait_id
        || checkpoint.wait_ids != expected_waits
        || !active_model_matches
        || !calls_match
    {
        return Err(StorageError::InvalidArgument(
            "LLM checkpoint is incompatible with approval batch".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_batch_fields(
    command: &PrepareToolApprovalBatchCommand,
    now: i64,
) -> StorageResult<()> {
    if command.calls.is_empty() || command.calls.len() > 32 {
        return Err(StorageError::InvalidArgument(
            "approval batch size is invalid".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut effect_ids = BTreeSet::new();
    let mut attempt_ids = BTreeSet::new();
    let mut total_bytes = 0usize;
    for (ordinal, call) in command.calls.iter().enumerate() {
        total_bytes = total_bytes.saturating_add(call.arguments_bytes.len());
        let ids_valid = [
            &call.tool_call_id,
            &call.effect_id,
            &call.effect_attempt_id,
            &call.binding_id,
            &call.tool_id,
            &call.tool_version,
            &call.call_digest,
            &call.descriptor_digest,
            &call.implementation_digest,
            &call.effect_operation_key,
            &call.effect_idempotency_key,
        ]
        .iter()
        .all(|value| !value.is_empty() && value.len() <= 256);
        if !ids_valid
            || call.call_index != ordinal as u64
            || call.arguments_bytes.is_empty()
            || call.arguments_bytes.len() > 1024 * 1024
            || call.risk_summary.len() > 512
            || call.retry_policy.max_attempts == 0
            || call.retry_policy.max_attempts > 32
            || call.retry_policy.backoff_ms.len() > 31
            || !ids.insert(&call.tool_call_id)
            || !effect_ids.insert(&call.effect_id)
            || !attempt_ids.insert(&call.effect_attempt_id)
        {
            return Err(StorageError::InvalidArgument(
                "approval call is outside supported bounds".into(),
            ));
        }
    }
    if total_bytes > 4 * 1024 * 1024
        || [
            &command.wait_id,
            &command.node_instance_id,
            &command.originating_attempt_id,
            &command.model_call_id,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256)
        || command
            .calls
            .iter()
            .any(|call| call.approval_expires_at < now)
    {
        return Err(StorageError::InvalidArgument(
            "approval batch is outside supported bounds".into(),
        ));
    }
    Ok(())
}

pub(super) fn prepare_digest(command: &PrepareToolApprovalBatchCommand) -> StorageResult<String> {
    canonical::hash(&json!({
        "schemaVersion": 1,
        "waitId": command.wait_id,
        "nodeInstanceId": command.node_instance_id,
        "originatingAttemptId": command.originating_attempt_id,
        "modelCallId": command.model_call_id,
        "checkpointDigest": command.checkpoint.checksum,
        "policyVersion": TOOL_CALL_POLICY_VERSION,
        "calls": command.calls.iter().map(|call| json!({
            "toolCallId": call.tool_call_id,
            "effectId": call.effect_id,
            "effectAttemptId": call.effect_attempt_id,
            "providerCallId": call.provider_call_id,
            "callIndex": call.call_index,
            "callDigest": call.call_digest,
            "argumentsBytes": call.arguments_bytes,
            "classification": call.effect_classification,
            "operationKey": call.effect_operation_key,
            "idempotencyKey": call.effect_idempotency_key,
            "retryPolicy": call.retry_policy,
            "riskSummary": call.risk_summary,
            "approvalExpiresAt": call.approval_expires_at,
        })).collect::<Vec<_>>(),
    }))
    .map_err(Into::into)
}

async fn count_calls<C: ConnectionTrait>(
    connection: &C,
    column: &str,
    value: &str,
) -> StorageResult<u64> {
    let statement = match column {
        "model_call_id" => "SELECT COUNT(*) AS count FROM tool_calls WHERE model_call_id = ?",
        "node_instance_id" => "SELECT COUNT(*) AS count FROM tool_calls WHERE node_instance_id = ?",
        _ => return Err(StorageError::Integrity("unknown tool count scope".into())),
    };
    let count: i64 = connection
        .query_one_raw(sql(statement, vec![value.into()]))
        .await?
        .expect("count query returns a row")
        .try_get("", "count")?;
    u64::try_from(count).map_err(|_| StorageError::Integrity("invalid tool-call count".into()))
}

pub(super) fn to_i64(value: u64, name: &str) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| StorageError::InvalidArgument(format!("{name} is too large")))
}
