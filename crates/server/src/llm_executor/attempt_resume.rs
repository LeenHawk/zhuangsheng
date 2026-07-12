use std::collections::BTreeSet;

use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    graph::LlmNodeExecutionSnapshot,
    llm::{
        EffectAttemptFence, LlmLogicalCallStatus, LlmLoopCheckpoint, LlmRequestBuildInput,
        LoadLlmResumeStateCommand, build_llm_request, context::ContextAssemblyOutput,
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::finalize_failure;

use super::{
    LocalLlmExecutor,
    count_resume::resume_active_count,
    model_call::{CompletedModelCall, ModelCallResult},
    model_completed_resume::reconstruct_completed_model_call,
    model_retry::{RetryModelCallInput, retry_model_call},
    tool_dispatch::{ToolDispatchResult, dispatch_resumed_tool_batch},
    tool_retry::prepare_tool_retries,
};

pub(super) enum AttemptResume {
    Continue(Box<ResumeLoopState>),
    Terminal(LlmAttemptExecution),
}

#[derive(Default)]
pub(super) struct ResumeLoopState {
    pub transcript_tail: Vec<zhuangsheng_core::llm::ir::LlmTurnItemIr>,
    pub prior_checkpoint: Option<LlmLoopCheckpoint>,
    pub recovered_completed: Option<CompletedModelCall>,
    pub output_repairs_used: u64,
    pub retry_ready_count_call: Option<zhuangsheng_core::llm::RetryReadyResumeCountCall>,
    pub completed_count_call: Option<zhuangsheng_core::llm::CompletedResumeCountCall>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn resume_attempt(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    execution: &LlmNodeExecutionSnapshot,
    assembly: &ContextAssemblyOutput,
    base_transcript_len: usize,
    credential: Option<&SecretValue>,
    reserved_output: u64,
    now: i64,
) -> Result<AttemptResume, ApplicationError> {
    let resume = executor
        .store
        .load_llm_resume_state(LoadLlmResumeStateCommand {
            node_instance_id: attempt.node_instance_id.clone(),
            fence: fence(attempt),
        })
        .await
        .map_err(ApplicationError::from)?;
    let Some(mut state) = resume else {
        return Ok(AttemptResume::Continue(Box::default()));
    };
    let mut output = ResumeLoopState {
        output_repairs_used: state.output_repairs_used,
        ..ResumeLoopState::default()
    };
    if let Some(resume) = resume_active_count(&mut state, &mut output, base_transcript_len)? {
        return Ok(resume);
    }
    let active_status = state
        .checkpoint
        .active_model_effect
        .as_ref()
        .map(|active| active.status)
        .ok_or(ApplicationError::Internal)?;
    match active_status {
        LlmLogicalCallStatus::RetryReady => {
            if state.pending_output_repair.is_some() {
                return Err(ApplicationError::Internal);
            }
            let retry = state
                .retry_ready_model_call
                .take()
                .ok_or(ApplicationError::Internal)?;
            if !state.prepared_tool_calls.is_empty() || !state.retry_ready_tool_calls.is_empty() {
                return Err(ApplicationError::Internal);
            }
            output.transcript_tail = tail(&state.transcript, base_transcript_len)?;
            let built = match build_llm_request(LlmRequestBuildInput {
                execution,
                context: assembly,
                registry_snapshot: &execution.tool_registry,
                tool_descriptors: &execution.tool_descriptors,
                transcript_tail: &output.transcript_tail,
                continuation: state.checkpoint.continuation_ref.as_ref(),
                approved_hosted_bindings: &BTreeSet::new(),
                model_call_no: state.checkpoint.model_call_no,
            }) {
                Ok(output) => output,
                Err(error) => {
                    return Ok(AttemptResume::Terminal(finalize_failure(
                        error.code,
                        &error.message,
                    )));
                }
            };
            output.recovered_completed = Some(
                match retry_model_call(
                    executor,
                    RetryModelCallInput {
                        attempt,
                        execution,
                        resume: retry,
                        checkpoint: state.checkpoint,
                        built,
                        credential,
                        reserved_output,
                        now,
                    },
                )
                .await?
                {
                    ModelCallResult::Completed(completed) => *completed,
                    ModelCallResult::Terminal(result) => {
                        return Ok(AttemptResume::Terminal(result));
                    }
                    ModelCallResult::Reassemble { .. } => {
                        return Err(ApplicationError::Internal);
                    }
                },
            );
        }
        LlmLogicalCallStatus::Completed => {
            if state.retry_ready_model_call.is_some() {
                return Err(ApplicationError::Internal);
            }
            if let Some(pending) = state.pending_output_repair.take() {
                if pending.repair_no != output.output_repairs_used
                    || !state.checkpoint.current_batch.is_empty()
                    || !state.prepared_tool_calls.is_empty()
                    || !state.retry_ready_tool_calls.is_empty()
                {
                    return Err(ApplicationError::Internal);
                }
                output.transcript_tail = tail(&state.transcript, base_transcript_len)?;
                output.prior_checkpoint = Some(state.checkpoint);
            } else if state.checkpoint.current_batch.is_empty() {
                output.recovered_completed = Some(
                    match reconstruct_completed_model_call(
                        execution,
                        assembly,
                        base_transcript_len,
                        state,
                    ) {
                        Ok(completed) => completed,
                        Err(error) => {
                            return Ok(AttemptResume::Terminal(finalize_failure(
                                error.code,
                                &error.message,
                            )));
                        }
                    },
                );
            } else {
                let state = prepare_tool_retries(executor, attempt, state, now).await?;
                let settled =
                    match dispatch_resumed_tool_batch(executor, attempt, execution, state, now)
                        .await?
                    {
                        ToolDispatchResult::Settled(settled) => *settled,
                        ToolDispatchResult::Terminal(result) => {
                            return Ok(AttemptResume::Terminal(result));
                        }
                    };
                output.transcript_tail = tail(&settled.transcript, base_transcript_len)?;
                output.prior_checkpoint = Some(settled.checkpoint);
            }
        }
        _ => {
            return Ok(AttemptResume::Terminal(finalize_failure(
                "llm_model_resume_state_invalid",
                "active model call is not resumable",
            )));
        }
    }
    Ok(AttemptResume::Continue(Box::new(output)))
}

pub(super) fn tail(
    transcript: &[zhuangsheng_core::llm::ir::LlmTurnItemIr],
    base: usize,
) -> Result<Vec<zhuangsheng_core::llm::ir::LlmTurnItemIr>, ApplicationError> {
    transcript
        .get(base..)
        .map(<[_]>::to_vec)
        .ok_or(ApplicationError::Internal)
}

fn fence(attempt: &ClaimedAttempt) -> EffectAttemptFence {
    EffectAttemptFence {
        invoking_node_attempt_id: attempt.attempt_id.clone(),
        worker_id: attempt.worker_id.clone(),
        lease_fence: attempt.lease_fence,
        run_control_epoch: attempt.run_control_epoch,
    }
}
