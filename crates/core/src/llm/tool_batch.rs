use thiserror::Error;

use crate::{
    canonical,
    graph::{ApprovalRequiredAction, LlmNodeExecutionSnapshot},
};

use super::{
    EffectRetryPolicy, LlmLoopCheckpoint, PrepareToolApprovalBatchCommand, PrepareToolApprovalCall,
    ResolvedRequestTool, TOOL_CALL_POLICY_VERSION, ToolCallCheckpoint, ToolCallCheckpointStatus,
    ToolCallDigestMaterial, ir::LlmTurnItemIr,
};

pub enum InitialToolBatchPlan {
    NoCalls,
    Approval(PrepareToolApprovalBatchCommand),
    Executable(ExecutableToolBatchPlan),
}

pub struct ExecutableToolBatchPlan {
    pub model_call_id: String,
    pub calls: Vec<PrepareToolApprovalCall>,
    pub checkpoint: LlmLoopCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct ToolBatchPlanError {
    pub code: &'static str,
    pub message: String,
}

impl ToolBatchPlanError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub struct InitialToolBatchInput<'a> {
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub request_tools: &'a [ResolvedRequestTool],
    pub response_items: &'a [LlmTurnItemIr],
    pub model_call_id: &'a str,
    pub node_instance_id: &'a str,
    pub originating_attempt_id: &'a str,
    pub checkpoint: LlmLoopCheckpoint,
    pub now_ms: i64,
}

pub fn plan_initial_tool_batch(
    input: InitialToolBatchInput<'_>,
) -> Result<InitialToolBatchPlan, ToolBatchPlanError> {
    let calls: Vec<_> = input
        .response_items
        .iter()
        .filter_map(|item| match item {
            LlmTurnItemIr::AssistantToolCall { call, .. } => Some(call),
            _ => None,
        })
        .collect();
    if calls.is_empty() {
        return Ok(InitialToolBatchPlan::NoCalls);
    }
    let max_calls = input.execution.limits.max_tool_calls.ok_or_else(|| {
        ToolBatchPlanError::new("tool_call_limit_missing", "tool-call limit is not pinned")
    })?;
    let used = input
        .checkpoint
        .tool_calls_used
        .checked_add(calls.len() as u64)
        .filter(|used| *used <= max_calls)
        .ok_or_else(|| {
            ToolBatchPlanError::new("tool_call_limit_exceeded", "tool-call limit exceeded")
        })?;
    let wait_id = format!("wait_tool_approval_{}", input.model_call_id);
    let mut plans = Vec::with_capacity(calls.len());
    let mut checkpoints = Vec::with_capacity(calls.len());
    let mut has_approval = false;
    for (index, call) in calls.into_iter().enumerate() {
        let resolved = input
            .request_tools
            .iter()
            .find(|tool| tool.exposed_name == call.name)
            .ok_or_else(|| {
                ToolBatchPlanError::new(
                    "tool_binding_unknown",
                    "model requested a tool that was not exposed by this request",
                )
            })?;
        if resolved.requires_approval
            && resolved
                .grant
                .failure_policy
                .as_ref()
                .is_some_and(|policy| policy.approval_required == ApprovalRequiredAction::FailNode)
        {
            return Err(ToolBatchPlanError::new(
                "tool_approval_policy_failed",
                "tool policy requires node failure instead of approval",
            ));
        }
        has_approval |= resolved.requires_approval;
        let arguments_bytes = canonical::to_vec(&call.arguments).map_err(|error| {
            ToolBatchPlanError::new("tool_arguments_invalid", error.to_string())
        })?;
        let material = ToolCallDigestMaterial {
            binding_id: resolved.binding_id.clone(),
            tool_id: resolved.descriptor.descriptor.tool_id.clone(),
            tool_version: resolved.descriptor.descriptor.version.clone(),
            arguments: call.arguments.clone(),
            grant: resolved.grant.clone(),
            descriptor_digest: resolved.descriptor.descriptor_digest.clone(),
            schema_compilation_digests: resolved.descriptor.schema_compilation_digests.clone(),
            implementation_digest: resolved.descriptor.implementation_digest.clone(),
            policy_version: TOOL_CALL_POLICY_VERSION,
        };
        let call_digest = material.digest().map_err(|error| {
            ToolBatchPlanError::new("tool_call_digest_failed", error.to_string())
        })?;
        let tool_call_id = format!("toolcall_{}_{}", input.model_call_id, index);
        let effect_id = format!("tooleffect_{}_{}", input.model_call_id, index);
        let effect_attempt_id = format!("tooleffectattempt_{}_{}", input.model_call_id, index);
        let retry_policy = retry_policy(resolved)?;
        plans.push(PrepareToolApprovalCall {
            tool_call_id: tool_call_id.clone(),
            effect_id,
            effect_attempt_id,
            provider_call_id: call.provider_call_id.clone(),
            call_index: index as u64,
            binding_id: resolved.binding_id.clone(),
            tool_id: resolved.descriptor.descriptor.tool_id.clone(),
            tool_version: resolved.descriptor.descriptor.version.clone(),
            call_digest: call_digest.clone(),
            arguments_bytes,
            descriptor_digest: resolved.descriptor.descriptor_digest.clone(),
            schema_compilation_digests: resolved.descriptor.schema_compilation_digests.clone(),
            implementation_digest: resolved.descriptor.implementation_digest.clone(),
            effect_classification: resolved.descriptor.descriptor.effect.classification,
            effect_operation_key: resolved.descriptor.descriptor.effect.operation_key.clone(),
            descriptor_requires_approval: resolved.descriptor.descriptor.effect.requires_approval,
            effect_idempotency_key: format!("tool:{tool_call_id}:effect"),
            retry_policy,
            risk_summary: if resolved.requires_approval {
                format!("Tool '{}' requests approval", resolved.exposed_name)
            } else {
                String::new()
            },
            approval_expires_at: input.now_ms.saturating_add(15 * 60 * 1000),
        });
        checkpoints.push(ToolCallCheckpoint {
            tool_call_id,
            call_index: index as u64,
            call_digest,
            status: if resolved.requires_approval {
                ToolCallCheckpointStatus::AwaitingApproval
            } else {
                ToolCallCheckpointStatus::Validated
            },
            effect_id: None,
            output_ref: None,
            wait_id: Some(wait_id.clone()),
        });
    }
    let mut checkpoint = input.checkpoint;
    checkpoint.last_updated_by_attempt_id = input.originating_attempt_id.into();
    if !has_approval {
        for call in &mut checkpoints {
            call.status = ToolCallCheckpointStatus::Requested;
            call.wait_id = None;
        }
        checkpoint.current_batch = checkpoints;
        checkpoint = checkpoint.seal().map_err(|error| {
            ToolBatchPlanError::new("tool_checkpoint_invalid", error.to_string())
        })?;
        return Ok(InitialToolBatchPlan::Executable(ExecutableToolBatchPlan {
            model_call_id: input.model_call_id.into(),
            calls: plans,
            checkpoint,
        }));
    }
    checkpoint.current_batch = checkpoints;
    checkpoint.tool_calls_used = used;
    checkpoint.effect_watermark = wait_id.clone();
    checkpoint.wait_ids.push(wait_id.clone());
    checkpoint = checkpoint
        .seal()
        .map_err(|error| ToolBatchPlanError::new("tool_checkpoint_invalid", error.to_string()))?;
    Ok(InitialToolBatchPlan::Approval(
        PrepareToolApprovalBatchCommand {
            wait_id,
            node_instance_id: input.node_instance_id.into(),
            originating_attempt_id: input.originating_attempt_id.into(),
            model_call_id: input.model_call_id.into(),
            calls: plans,
            checkpoint,
        },
    ))
}

fn retry_policy(tool: &ResolvedRequestTool) -> Result<EffectRetryPolicy, ToolBatchPlanError> {
    let Some(policy) = &tool.grant.failure_policy else {
        return Ok(EffectRetryPolicy {
            max_attempts: 1,
            backoff_ms: Vec::new(),
        });
    };
    let max_attempts = u32::try_from(policy.max_attempts).map_err(|_| {
        ToolBatchPlanError::new("tool_retry_policy_invalid", "tool retry limit is too large")
    })?;
    if max_attempts == 0 || max_attempts > 32 || policy.retry_backoff_ms.len() > 31 {
        return Err(ToolBatchPlanError::new(
            "tool_retry_policy_invalid",
            "tool retry policy is outside supported bounds",
        ));
    }
    Ok(EffectRetryPolicy {
        max_attempts,
        backoff_ms: policy.retry_backoff_ms.clone(),
    })
}
