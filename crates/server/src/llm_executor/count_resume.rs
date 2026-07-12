use zhuangsheng_core::{
    application::ApplicationError,
    llm::{LlmLogicalCallStatus, LlmResumeState},
};

use crate::llm_executor_support::finalize_failure;

use super::attempt_resume::{AttemptResume, ResumeLoopState, tail};

pub(super) fn resume_active_count(
    state: &mut LlmResumeState,
    output: &mut ResumeLoopState,
    base_transcript_len: usize,
) -> Result<Option<AttemptResume>, ApplicationError> {
    if state.checkpoint.count_calls_used <= state.checkpoint.model_calls_used {
        return Ok(None);
    }
    let status = state
        .checkpoint
        .active_count_effect
        .as_ref()
        .map(|active| active.status)
        .ok_or(ApplicationError::Internal)?;
    if !state.checkpoint.current_batch.is_empty()
        || state.pending_output_repair.is_some()
        || state.retry_ready_model_call.is_some()
        || !state.prepared_tool_calls.is_empty()
        || !state.retry_ready_tool_calls.is_empty()
    {
        return Err(ApplicationError::Internal);
    }
    match status {
        LlmLogicalCallStatus::Completed => {
            if state.retry_ready_count_call.is_some() {
                return Err(ApplicationError::Internal);
            }
            output.completed_count_call = Some(
                state
                    .completed_count_call
                    .take()
                    .ok_or(ApplicationError::Internal)?,
            );
        }
        LlmLogicalCallStatus::RetryReady => {
            if state.completed_count_call.is_some() {
                return Err(ApplicationError::Internal);
            }
            output.retry_ready_count_call = Some(
                state
                    .retry_ready_count_call
                    .take()
                    .ok_or(ApplicationError::Internal)?,
            );
        }
        _ => {
            return Ok(Some(AttemptResume::Terminal(finalize_failure(
                "llm_count_resume_state_invalid",
                "active token count is not resumable",
            ))));
        }
    }
    output.transcript_tail = tail(&state.transcript, base_transcript_len)?;
    output.prior_checkpoint = Some(state.checkpoint.clone());
    Ok(Some(AttemptResume::Continue(Box::new(std::mem::take(
        output,
    )))))
}
