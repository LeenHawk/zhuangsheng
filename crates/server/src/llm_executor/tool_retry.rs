use zhuangsheng_core::{
    application::ApplicationError,
    llm::{
        EffectAttemptFence, LlmResumeState, PrepareToolCallRetryCommand, PreparedResumeToolCall,
        ToolCallCheckpointStatus,
    },
    scheduler::ClaimedAttempt,
};

use crate::llm_executor_support::new_id;

use super::LocalLlmExecutor;

pub(super) async fn prepare_tool_retries(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    mut state: LlmResumeState,
    now: i64,
) -> Result<LlmResumeState, ApplicationError> {
    let fence = EffectAttemptFence {
        invoking_node_attempt_id: attempt.attempt_id.clone(),
        worker_id: attempt.worker_id.clone(),
        lease_fence: attempt.lease_fence,
        run_control_epoch: attempt.run_control_epoch,
    };
    for call in std::mem::take(&mut state.retry_ready_tool_calls) {
        let effect_attempt_id = new_id("tooleffectattempt");
        let checkpoint_call = state
            .checkpoint
            .current_batch
            .iter_mut()
            .find(|checkpoint| checkpoint.tool_call_id == call.tool_call_id)
            .ok_or(ApplicationError::Internal)?;
        checkpoint_call.status = ToolCallCheckpointStatus::Prepared;
        checkpoint_call.effect_id = Some(call.effect_id.clone());
        checkpoint_call.output_ref = None;
        checkpoint_call.wait_id = None;
        state.checkpoint.last_updated_by_attempt_id = attempt.attempt_id.clone();
        state.checkpoint.effect_watermark = effect_attempt_id.clone();
        state.checkpoint = state
            .checkpoint
            .seal()
            .map_err(|_| ApplicationError::Internal)?;
        executor
            .store
            .prepare_tool_call_retry(
                PrepareToolCallRetryCommand {
                    tool_call_id: call.tool_call_id.clone(),
                    effect_attempt_id: effect_attempt_id.clone(),
                    fence: fence.clone(),
                    checkpoint: state.checkpoint.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        state.prepared_tool_calls.push(PreparedResumeToolCall {
            tool_call_id: call.tool_call_id,
            effect_id: call.effect_id,
            effect_attempt_id,
            model_call_id: call.model_call_id,
            call_index: call.call_index,
            binding_id: call.binding_id,
            tool_id: call.tool_id,
            tool_version: call.tool_version,
            arguments: call.arguments,
            effect_idempotency_key: call.effect_idempotency_key,
        });
    }
    state
        .prepared_tool_calls
        .sort_by_key(|call| call.call_index);
    Ok(state)
}
