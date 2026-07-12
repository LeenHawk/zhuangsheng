use std::time::Duration;

use zhuangsheng_core::{
    application::{
        ApplicationError,
        tool::{ToolExecutionContext, ToolInvocation, ToolOutputPart},
    },
    canonical,
    graph::{LlmNodeExecutionSnapshot, ToolFailureAction},
    llm::{
        EffectAttemptFence, ExecutableToolBatchPlan, FinishToolCallCommand, LlmLoopCheckpoint,
        LlmResumeState, PrepareToolCallCommand, SettleToolBatchCommand, SettledToolBatch,
        StartToolCallCommand, ToolCallCheckpointStatus, ToolCallOutcome,
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::finalize_failure;

use super::LocalLlmExecutor;

pub(super) enum ToolDispatchResult {
    Settled(Box<SettledToolBatch>),
    Terminal(LlmAttemptExecution),
}

pub(super) async fn dispatch_tool_batch(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    execution: &LlmNodeExecutionSnapshot,
    batch: ExecutableToolBatchPlan,
    now: i64,
) -> Result<ToolDispatchResult, ApplicationError> {
    let mut prepared = Vec::with_capacity(batch.calls.len());
    for call in &batch.calls {
        let descriptor = execution
            .tool_descriptors
            .iter()
            .find(|item| {
                item.descriptor.tool_id == call.tool_id
                    && item.descriptor.version == call.tool_version
            })
            .cloned()
            .ok_or(ApplicationError::Internal)?;
        let grant = execution
            .tools
            .iter()
            .find(|grant| grant.binding_id == call.binding_id)
            .cloned()
            .ok_or(ApplicationError::Internal)?;
        let Some(tool_executor) = executor
            .tools
            .resolve(&descriptor.executor_key, &descriptor.implementation_digest)
        else {
            return Ok(ToolDispatchResult::Terminal(finalize_failure(
                "tool_executor_unavailable",
                "pinned tool implementation is unavailable",
            )));
        };
        let arguments = serde_json::from_slice(&call.arguments_bytes)
            .map_err(|_| ApplicationError::Internal)?;
        prepared.push((descriptor, grant, tool_executor, arguments));
    }
    let fence = fence(attempt);
    let model_call_id = batch.model_call_id;
    if model_call_id.is_empty() {
        return Ok(ToolDispatchResult::Terminal(finalize_failure(
            "tool_batch_identity_invalid",
            "tool batch identity is invalid",
        )));
    }
    let mut checkpoint = batch.checkpoint;
    for (call, (descriptor, grant, tool_executor, arguments)) in
        batch.calls.into_iter().zip(prepared)
    {
        checkpoint.tool_calls_used = checkpoint
            .tool_calls_used
            .checked_add(1)
            .ok_or(ApplicationError::Internal)?;
        transition_call(
            &mut checkpoint,
            &call.tool_call_id,
            ToolCallCheckpointStatus::Prepared,
            Some(&call.effect_id),
        )?;
        checkpoint.last_updated_by_attempt_id = attempt.attempt_id.clone();
        checkpoint.effect_watermark = call.effect_attempt_id.clone();
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        executor
            .store
            .prepare_tool_call(
                PrepareToolCallCommand {
                    tool_call_id: call.tool_call_id.clone(),
                    effect_id: call.effect_id.clone(),
                    effect_attempt_id: call.effect_attempt_id.clone(),
                    node_instance_id: attempt.node_instance_id.clone(),
                    originating_attempt_id: attempt.attempt_id.clone(),
                    model_call_id: model_call_id.clone(),
                    provider_call_id: call.provider_call_id.clone(),
                    call_index: call.call_index,
                    binding_id: call.binding_id.clone(),
                    tool_id: call.tool_id.clone(),
                    tool_version: call.tool_version.clone(),
                    call_digest: call.call_digest.clone(),
                    arguments_bytes: call.arguments_bytes.clone(),
                    descriptor_digest: call.descriptor_digest.clone(),
                    schema_compilation_digests: call.schema_compilation_digests.clone(),
                    implementation_digest: call.implementation_digest.clone(),
                    effect_classification: call.effect_classification,
                    effect_operation_key: call.effect_operation_key.clone(),
                    descriptor_requires_approval: call.descriptor_requires_approval,
                    effect_idempotency_key: call.effect_idempotency_key.clone(),
                    retry_policy: call.retry_policy.clone(),
                    checkpoint: checkpoint.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        transition_call(
            &mut checkpoint,
            &call.tool_call_id,
            ToolCallCheckpointStatus::Running,
            Some(&call.effect_id),
        )?;
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        executor
            .store
            .start_tool_call(
                StartToolCallCommand {
                    effect_attempt_id: call.effect_attempt_id.clone(),
                    fence: fence.clone(),
                    provider_request_id: None,
                    checkpoint: checkpoint.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        let invocation = ToolInvocation {
            run_id: attempt.run_id.clone(),
            node_instance_id: attempt.node_instance_id.clone(),
            tool_call_id: call.tool_call_id.clone(),
            binding_id: call.binding_id.clone(),
            tool_id: call.tool_id.clone(),
            tool_version: call.tool_version.clone(),
            arguments,
            effect_idempotency_key: call.effect_idempotency_key.clone(),
            grant: grant.clone(),
            descriptor: descriptor.clone(),
        };
        let outcome = tokio::time::timeout(
            Duration::from_millis(descriptor.descriptor.limits.timeout_ms),
            tool_executor.execute(ToolExecutionContext { invocation }),
        )
        .await;
        let terminal = encode_outcome(
            outcome,
            descriptor.descriptor.limits.max_llm_result_bytes,
            grant
                .failure_policy
                .as_ref()
                .is_some_and(|policy| policy.execution_error == ToolFailureAction::FailNode),
        );
        transition_call(
            &mut checkpoint,
            &call.tool_call_id,
            terminal.status,
            Some(&call.effect_id),
        )?;
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        checkpoint = executor
            .store
            .finish_tool_call(
                FinishToolCallCommand {
                    effect_attempt_id: call.effect_attempt_id,
                    fence: fence.clone(),
                    outcome: terminal.outcome,
                    checkpoint,
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        if terminal.handled {
            return Ok(ToolDispatchResult::Terminal(LlmAttemptExecution::Handled));
        }
        if terminal.fail_node {
            return Ok(ToolDispatchResult::Terminal(finalize_failure(
                "tool_execution_failed",
                "custom tool execution failed",
            )));
        }
    }
    let settled = executor
        .store
        .settle_tool_batch(
            SettleToolBatchCommand {
                node_instance_id: attempt.node_instance_id.clone(),
                model_call_id,
                fence,
                checkpoint,
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    Ok(ToolDispatchResult::Settled(Box::new(settled)))
}

pub(super) async fn dispatch_resumed_tool_batch(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    execution: &LlmNodeExecutionSnapshot,
    state: LlmResumeState,
    now: i64,
) -> Result<ToolDispatchResult, ApplicationError> {
    let fence = fence(attempt);
    let model_call_id = state
        .checkpoint
        .active_model_effect
        .as_ref()
        .map(|active| active.model_call_id.clone())
        .ok_or(ApplicationError::Internal)?;
    let mut checkpoint = state.checkpoint;
    for call in state.prepared_tool_calls {
        let descriptor = execution
            .tool_descriptors
            .iter()
            .find(|item| {
                item.descriptor.tool_id == call.tool_id
                    && item.descriptor.version == call.tool_version
            })
            .cloned()
            .ok_or(ApplicationError::Internal)?;
        let grant = execution
            .tools
            .iter()
            .find(|grant| grant.binding_id == call.binding_id)
            .cloned()
            .ok_or(ApplicationError::Internal)?;
        let Some(tool_executor) = executor
            .tools
            .resolve(&descriptor.executor_key, &descriptor.implementation_digest)
        else {
            return Ok(ToolDispatchResult::Terminal(finalize_failure(
                "tool_executor_unavailable",
                "pinned tool implementation is unavailable",
            )));
        };
        transition_call(
            &mut checkpoint,
            &call.tool_call_id,
            ToolCallCheckpointStatus::Running,
            Some(&call.effect_id),
        )?;
        checkpoint.effect_watermark = call.effect_attempt_id.clone();
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        executor
            .store
            .start_tool_call(
                StartToolCallCommand {
                    effect_attempt_id: call.effect_attempt_id.clone(),
                    fence: fence.clone(),
                    provider_request_id: None,
                    checkpoint: checkpoint.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        let invocation = ToolInvocation {
            run_id: attempt.run_id.clone(),
            node_instance_id: attempt.node_instance_id.clone(),
            tool_call_id: call.tool_call_id.clone(),
            binding_id: call.binding_id.clone(),
            tool_id: call.tool_id.clone(),
            tool_version: call.tool_version.clone(),
            arguments: call.arguments,
            effect_idempotency_key: call.effect_idempotency_key,
            grant: grant.clone(),
            descriptor: descriptor.clone(),
        };
        let outcome = tokio::time::timeout(
            Duration::from_millis(descriptor.descriptor.limits.timeout_ms),
            tool_executor.execute(ToolExecutionContext { invocation }),
        )
        .await;
        let terminal = encode_outcome(
            outcome,
            descriptor.descriptor.limits.max_llm_result_bytes,
            grant
                .failure_policy
                .as_ref()
                .is_some_and(|policy| policy.execution_error == ToolFailureAction::FailNode),
        );
        transition_call(
            &mut checkpoint,
            &call.tool_call_id,
            terminal.status,
            Some(&call.effect_id),
        )?;
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        checkpoint = executor
            .store
            .finish_tool_call(
                FinishToolCallCommand {
                    effect_attempt_id: call.effect_attempt_id,
                    fence: fence.clone(),
                    outcome: terminal.outcome,
                    checkpoint,
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        if terminal.handled {
            return Ok(ToolDispatchResult::Terminal(LlmAttemptExecution::Handled));
        }
        if terminal.fail_node {
            return Ok(ToolDispatchResult::Terminal(finalize_failure(
                "tool_execution_failed",
                "custom tool execution failed",
            )));
        }
    }
    let settled = executor
        .store
        .settle_tool_batch(
            SettleToolBatchCommand {
                node_instance_id: attempt.node_instance_id.clone(),
                model_call_id,
                fence,
                checkpoint,
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    Ok(ToolDispatchResult::Settled(Box::new(settled)))
}

struct EncodedOutcome {
    status: ToolCallCheckpointStatus,
    outcome: ToolCallOutcome,
    handled: bool,
    fail_node: bool,
}

fn encode_outcome(
    outcome: Result<
        Result<
            zhuangsheng_core::application::tool::ToolCallOutput,
            zhuangsheng_core::application::tool::ToolExecutionError,
        >,
        tokio::time::error::Elapsed,
    >,
    max_bytes: u64,
    policy_fails_node: bool,
) -> EncodedOutcome {
    match outcome {
        Ok(Ok(output)) => {
            let valid_llm_result = output.parts.iter().filter(|part| {
                matches!(part, ToolOutputPart::LlmResult { content } if !content.is_empty())
            }).count() == 1;
            let bytes = canonical::to_vec(&output).unwrap_or_default();
            if valid_llm_result && !bytes.is_empty() && bytes.len() as u64 <= max_bytes {
                return EncodedOutcome {
                    status: ToolCallCheckpointStatus::Completed,
                    outcome: ToolCallOutcome::Completed {
                        output_bytes: bytes,
                    },
                    handled: false,
                    fail_node: false,
                };
            }
            failed_outcome(
                "tool_output_invalid",
                "tool returned an invalid or oversized result",
                true,
            )
        }
        Ok(Err(error)) if error.outcome_unknown => EncodedOutcome {
            status: ToolCallCheckpointStatus::OutcomeUnknown,
            outcome: ToolCallOutcome::OutcomeUnknown {
                error_bytes: tool_error_bytes(&error.code, &error.safe_message),
            },
            handled: true,
            fail_node: false,
        },
        Ok(Err(error)) => failed_outcome(&error.code, &error.safe_message, policy_fails_node),
        Err(_) => failed_outcome(
            "tool_execution_timeout",
            "tool execution timed out",
            policy_fails_node,
        ),
    }
}

fn failed_outcome(code: &str, message: &str, fail_node: bool) -> EncodedOutcome {
    EncodedOutcome {
        status: ToolCallCheckpointStatus::Failed,
        outcome: ToolCallOutcome::Failed {
            error_bytes: tool_error_bytes(code, message),
        },
        handled: false,
        fail_node,
    }
}

fn tool_error_bytes(code: &str, safe_message: &str) -> Vec<u8> {
    canonical::to_vec(&serde_json::json!({
        "schemaVersion":1,
        "code":code,
        "safeMessage":safe_message,
    }))
    .unwrap_or_else(|_| b"{\"code\":\"tool_execution_failed\"}".to_vec())
}

fn transition_call(
    checkpoint: &mut LlmLoopCheckpoint,
    tool_call_id: &str,
    status: ToolCallCheckpointStatus,
    effect_id: Option<&str>,
) -> Result<(), ApplicationError> {
    let call = checkpoint
        .current_batch
        .iter_mut()
        .find(|call| call.tool_call_id == tool_call_id)
        .ok_or(ApplicationError::Internal)?;
    call.status = status;
    call.effect_id = effect_id.map(str::to_owned);
    call.wait_id = None;
    call.output_ref = None;
    Ok(())
}

fn fence(attempt: &ClaimedAttempt) -> EffectAttemptFence {
    EffectAttemptFence {
        invoking_node_attempt_id: attempt.attempt_id.clone(),
        worker_id: attempt.worker_id.clone(),
        lease_fence: attempt.lease_fence,
        run_control_epoch: attempt.run_control_epoch,
    }
}
