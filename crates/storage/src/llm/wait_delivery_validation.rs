use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    runtime::{SubmitWaitResponseCommand, ToolApprovalDecision, ToolApprovalDecisionKind},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
    runtime::add_object_ref,
};

use super::{tool_approval::ToolApprovalContinuation, wait_delivery::WaitContext};

pub(super) async fn validate_continuation<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    context: &WaitContext,
    continuation: &ToolApprovalContinuation,
) -> StorageResult<()> {
    if continuation.schema_version != 1
        || continuation.node_instance_id != context.node_instance_id
        || continuation.originating_attempt_id != context.node_attempt_id
        || continuation.calls.is_empty()
        || !continuation.calls.iter().any(|call| call.requires_approval)
    {
        return Err(StorageError::Integrity(
            "tool approval continuation is incompatible".into(),
        ));
    }
    let rows = connection
        .query_all_raw(sql(
            "SELECT id, model_call_id, call_digest, arguments_object_id, status FROM tool_calls WHERE node_instance_id = ? ORDER BY model_call_id, call_index",
            vec![context.node_instance_id.clone().into()],
        ))
        .await?;
    for plan in &continuation.calls {
        let row = rows
            .iter()
            .find(|row| row.try_get::<String>("", "id").as_deref() == Ok(&plan.tool_call_id))
            .ok_or_else(|| StorageError::Integrity("approval tool call is missing".into()))?;
        let expected_status = if plan.requires_approval {
            "awaiting_approval"
        } else {
            "validated"
        };
        if row.try_get::<String>("", "model_call_id")? != continuation.model_call_id
            || row.try_get::<String>("", "call_digest")? != plan.call_digest
            || row.try_get::<String>("", "arguments_object_id")? != plan.arguments_ref
            || row.try_get::<String>("", "status")? != expected_status
        {
            return Err(StorageError::Conflict("approval_tool_projection"));
        }
    }
    let open_kinds: Vec<String> = connection
        .query_all_raw(sql(
            "SELECT blocker_kind FROM wait_blockers WHERE wait_id = ? AND status = 'open' ORDER BY blocker_order",
            vec![command.wait_id.clone().into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "blocker_kind"))
        .collect::<Result<_, _>>()?;
    if open_kinds.is_empty() || open_kinds.iter().any(|kind| kind != "tool_call") {
        return Err(StorageError::Conflict("wait_response_kind"));
    }
    Ok(())
}

pub(super) async fn validate_decisions<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
    decisions: &[ToolApprovalDecision],
    continuation: &ToolApprovalContinuation,
    now: i64,
) -> StorageResult<Vec<ToolApprovalDecision>> {
    let blockers = connection.query_all_raw(sql(
        "SELECT blocker_id FROM wait_blockers WHERE wait_id = ? AND blocker_kind = 'tool_call' AND status = 'open' ORDER BY blocker_order",
        vec![wait_id.into()],
    )).await?;
    if blockers.len() != decisions.len() {
        return Err(StorageError::InvalidArgument(
            "approval response must cover every open blocker".into(),
        ));
    }
    let mut supplied = BTreeMap::new();
    for decision in decisions {
        if supplied
            .insert(decision.tool_call_id.clone(), decision)
            .is_some()
            || decision
                .reason
                .as_ref()
                .is_some_and(|reason| reason.len() > 512)
        {
            return Err(StorageError::InvalidArgument(
                "approval response contains duplicate or oversized decisions".into(),
            ));
        }
    }
    let mut ordered = Vec::with_capacity(blockers.len());
    for blocker in blockers {
        let id: String = blocker.try_get("", "blocker_id")?;
        let decision = supplied.get(&id).ok_or_else(|| {
            StorageError::InvalidArgument("approval response is missing a blocker".into())
        })?;
        let plan = continuation
            .calls
            .iter()
            .find(|call| call.tool_call_id == id && call.requires_approval)
            .ok_or_else(|| StorageError::Integrity("approval blocker has no plan".into()))?;
        if decision.call_digest != plan.call_digest || now >= plan.approval_expires_at {
            return Err(StorageError::InvalidArgument(
                "approval decision digest is stale or expired".into(),
            ));
        }
        ordered.push((*decision).clone());
    }
    Ok(ordered)
}

pub(super) async fn settle_decisions<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    continuation: &ToolApprovalContinuation,
    decisions: &[ToolApprovalDecision],
    now: i64,
) -> StorageResult<Vec<String>> {
    let mut refs = Vec::with_capacity(decisions.len());
    for decision in decisions {
        let plan = continuation
            .calls
            .iter()
            .find(|call| call.tool_call_id == decision.tool_call_id)
            .expect("validated approval plan");
        let bytes = canonical::to_vec(&json!({
            "schemaVersion": 1,
            "toolCallId": decision.tool_call_id,
            "callDigest": decision.call_digest,
            "decision": decision.decision,
            "reason": decision.reason,
            "actorKind": command.actor_kind,
            "actorId": command.actor_id,
            "policyVersion": zhuangsheng_core::llm::TOOL_CALL_POLICY_VERSION,
            "expiresAt": plan.approval_expires_at,
        }))?;
        let decision_ref = put_inline_object(connection, &bytes, now).await?;
        let status = if decision.decision == ToolApprovalDecisionKind::Approve {
            "satisfied"
        } else {
            "rejected"
        };
        if connection.execute_raw(sql(
            "UPDATE wait_blockers SET status = ?, decision_object_id = ? WHERE wait_id = ? AND blocker_kind = 'tool_call' AND blocker_id = ? AND status = 'open'",
            vec![status.into(), decision_ref.clone().into(), command.wait_id.clone().into(), decision.tool_call_id.clone().into()],
        )).await?.rows_affected() != 1 {
            return Err(StorageError::Conflict("approval_wait_blocker"));
        }
        add_object_ref(
            connection,
            &decision_ref,
            "node_wait",
            &command.wait_id,
            "decision",
            now,
        )
        .await?;
        refs.push(decision_ref);
    }
    Ok(refs)
}
