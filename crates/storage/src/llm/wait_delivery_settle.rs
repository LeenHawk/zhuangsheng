use std::collections::BTreeSet;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{LlmLoopCheckpoint, ToolCallCheckpointStatus},
    runtime::{ToolApprovalDecision, ToolApprovalDecisionKind},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
    runtime::{ResumeAttempt, create_resume_attempt as create_runtime_resume_attempt},
};

use super::{
    model_ledger_helpers::{add_ref, classification_name, persist_checkpoint},
    tool_approval::{ToolApprovalCallPlan, ToolApprovalContinuation},
    wait_delivery::WaitContext,
};

pub(super) async fn settle_executable_batch<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    delivery_id: &str,
    continuation: &ToolApprovalContinuation,
    decisions: &[ToolApprovalDecision],
    checkpoint: &mut LlmLoopCheckpoint,
    now: i64,
) -> StorageResult<(Vec<String>, Vec<String>, Option<String>)> {
    let resume_attempt_id = create_resume_attempt(connection, context, delivery_id, now).await?;
    let rejected: BTreeSet<_> = decisions
        .iter()
        .filter(|decision| decision.decision == ToolApprovalDecisionKind::Reject)
        .map(|decision| decision.tool_call_id.as_str())
        .collect();
    let mut prepared = Vec::new();
    let mut denied = Vec::new();
    let mut watermark = delivery_id.to_owned();
    for plan in &continuation.calls {
        if rejected.contains(plan.tool_call_id.as_str()) {
            deny_call(connection, plan, checkpoint, now).await?;
            denied.push(plan.tool_call_id.clone());
        } else {
            prepare_effect(connection, plan, &resume_attempt_id, checkpoint, now).await?;
            watermark = plan.effect_attempt_id.clone();
            prepared.push(plan.tool_call_id.clone());
        }
    }
    checkpoint.last_updated_by_attempt_id = resume_attempt_id.clone();
    checkpoint.effect_watermark = watermark;
    *checkpoint = checkpoint.clone().seal()?;
    persist_checkpoint(connection, checkpoint, now).await?;
    resolve_wait_owner(connection, context, now).await?;
    Ok((prepared, denied, Some(resume_attempt_id)))
}

pub(super) async fn settle_failed_batch<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    continuation: &ToolApprovalContinuation,
    decisions: &[ToolApprovalDecision],
    checkpoint: &mut LlmLoopCheckpoint,
    now: i64,
) -> StorageResult<(Vec<String>, Vec<String>, Option<String>)> {
    let rejected: BTreeSet<_> = decisions
        .iter()
        .filter(|decision| decision.decision == ToolApprovalDecisionKind::Reject)
        .map(|decision| decision.tool_call_id.as_str())
        .collect();
    let mut denied = Vec::new();
    for plan in &continuation.calls {
        if rejected.contains(plan.tool_call_id.as_str()) {
            deny_call(connection, plan, checkpoint, now).await?;
            denied.push(plan.tool_call_id.clone());
        } else {
            cancel_unstarted_call(connection, plan, checkpoint).await?;
        }
    }
    checkpoint.effect_watermark = format!("approval-failed:{}", context.node_attempt_id);
    *checkpoint = checkpoint.clone().seal()?;
    persist_checkpoint(connection, checkpoint, now).await?;
    Ok((Vec::new(), denied, None))
}

async fn prepare_effect<C: ConnectionTrait>(
    connection: &C,
    plan: &ToolApprovalCallPlan,
    invoking_attempt_id: &str,
    checkpoint: &mut LlmLoopCheckpoint,
    now: i64,
) -> StorageResult<()> {
    let retry_json = canonical::to_string(&plan.retry_policy)?;
    connection.execute(sql(
        "INSERT INTO effects (id, node_instance_id, tool_call_id, effect_kind, classification, operation_key, idempotency_key, retry_policy_json, status, created_at) SELECT ?, node_instance_id, id, 'custom_tool', ?, ?, ?, ?, 'pending', ? FROM tool_calls WHERE id = ?",
        vec![
            plan.effect_id.clone().into(),
            classification_name(plan.classification).into(),
            plan.operation_key.clone().into(),
            plan.idempotency_key.clone().into(),
            retry_json.into(),
            now.into(),
            plan.tool_call_id.clone().into(),
        ],
    )).await?;
    connection.execute(sql(
        "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, 1, 'prepared', ?)",
        vec![
            plan.effect_attempt_id.clone().into(),
            plan.effect_id.clone().into(),
            invoking_attempt_id.into(),
            plan.arguments_ref.clone().into(),
        ],
    )).await?;
    if connection.execute(sql(
        "UPDATE tool_calls SET status = 'prepared' WHERE id = ? AND status IN ('validated','awaiting_approval')",
        vec![plan.tool_call_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("approval_tool_status"));
    }
    add_ref(
        connection,
        &plan.arguments_ref,
        "effect_attempt",
        &plan.effect_attempt_id,
        "request",
        now,
    )
    .await?;
    let call = checkpoint
        .current_batch
        .iter_mut()
        .find(|call| call.tool_call_id == plan.tool_call_id)
        .ok_or_else(|| StorageError::Integrity("approval checkpoint call is missing".into()))?;
    call.status = ToolCallCheckpointStatus::Prepared;
    call.effect_id = Some(plan.effect_id.clone());
    call.wait_id = None;
    Ok(())
}

async fn deny_call<C: ConnectionTrait>(
    connection: &C,
    plan: &ToolApprovalCallPlan,
    checkpoint: &mut LlmLoopCheckpoint,
    now: i64,
) -> StorageResult<()> {
    let error = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "code": "tool_call_denied",
        "safeMessage": "tool call was rejected",
        "toolCallId": plan.tool_call_id,
    }))?;
    let error_ref = put_inline_object(connection, &error, now).await?;
    if connection.execute(sql(
        "UPDATE tool_calls SET status = 'denied', error_object_id = ?, finished_at = ? WHERE id = ? AND status = 'awaiting_approval'",
        vec![error_ref.clone().into(), now.into(), plan.tool_call_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("approval_tool_status"));
    }
    add_ref(
        connection,
        &error_ref,
        "tool_call",
        &plan.tool_call_id,
        "error",
        now,
    )
    .await?;
    let call = checkpoint
        .current_batch
        .iter_mut()
        .find(|call| call.tool_call_id == plan.tool_call_id)
        .ok_or_else(|| StorageError::Integrity("approval checkpoint call is missing".into()))?;
    call.status = ToolCallCheckpointStatus::Denied;
    call.effect_id = None;
    call.wait_id = None;
    Ok(())
}

async fn cancel_unstarted_call<C: ConnectionTrait>(
    connection: &C,
    plan: &ToolApprovalCallPlan,
    checkpoint: &mut LlmLoopCheckpoint,
) -> StorageResult<()> {
    if connection.execute(sql(
        "UPDATE tool_calls SET status = 'cancelled_before_start' WHERE id = ? AND status IN ('validated','awaiting_approval')",
        vec![plan.tool_call_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("approval_tool_status"));
    }
    let call = checkpoint
        .current_batch
        .iter_mut()
        .find(|call| call.tool_call_id == plan.tool_call_id)
        .ok_or_else(|| StorageError::Integrity("approval checkpoint call is missing".into()))?;
    call.status = ToolCallCheckpointStatus::CancelledBeforeStart;
    call.effect_id = None;
    call.wait_id = None;
    Ok(())
}

async fn create_resume_attempt<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    delivery_id: &str,
    now: i64,
) -> StorageResult<String> {
    let idempotency_key = format!("wait:{delivery_id}:resume");
    create_runtime_resume_attempt(
        connection,
        ResumeAttempt {
            node_instance_id: &context.node_instance_id,
            source_attempt_id: &context.node_attempt_id,
            run_id: &context.run_id,
            control_epoch: context.control_epoch,
            idempotency_key: &idempotency_key,
        },
        now,
    )
    .await
}

async fn resolve_wait_owner<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    now: i64,
) -> StorageResult<()> {
    if connection.execute(sql(
        "UPDATE node_instances SET status = 'ready', updated_at = ? WHERE id = ? AND status = 'waiting'",
        vec![now.into(), context.node_instance_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("approval_wait_owner"));
    }
    Ok(())
}
