use zhuangsheng_core::{
    application::ApplicationError,
    graph::LlmNodeExecutionSnapshot,
    llm::{
        EffectAttemptFence, MemoryToolBatchInput, MemoryToolBatchPlan, SettleToolBatchCommand,
        SettledToolBatch, plan_memory_tool_batch,
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::finalize_failure;

use super::{LocalLlmExecutor, model_call::CompletedModelCall};

pub(super) enum MemoryDispatchResult {
    NoCalls,
    Settled(Box<SettledToolBatch>),
    Terminal(LlmAttemptExecution),
}

pub(super) async fn dispatch_memory_tool_batch(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    execution: &LlmNodeExecutionSnapshot,
    completed: &CompletedModelCall,
    now: i64,
) -> Result<MemoryDispatchResult, ApplicationError> {
    let plan = match plan_memory_tool_batch(MemoryToolBatchInput {
        tools: &completed.resolved_memory_tools,
        response_items: &completed.decoded.response.items,
        model_call_id: &completed.model_call_id,
        node_instance_id: &attempt.node_instance_id,
        originating_attempt_id: &attempt.attempt_id,
        checkpoint: completed.checkpoint.clone(),
        max_tool_calls: execution
            .limits
            .max_tool_calls
            .ok_or(ApplicationError::Internal)?,
    }) {
        Ok(plan) => plan,
        Err(error) => {
            return Ok(MemoryDispatchResult::Terminal(finalize_failure(
                error.code,
                &error.message,
            )));
        }
    };
    match plan {
        None => Ok(MemoryDispatchResult::NoCalls),
        Some(MemoryToolBatchPlan::Proposal(command)) => {
            executor
                .store
                .prepare_memory_proposal_tool_batch(command, now)
                .await
                .map_err(ApplicationError::from)?;
            Ok(MemoryDispatchResult::Terminal(LlmAttemptExecution::Handled))
        }
        Some(MemoryToolBatchPlan::Search(command)) => {
            let model_call_id = command.model_call_id.clone();
            let result = executor
                .store
                .execute_memory_search_tool_batch(command, now)
                .await
                .map_err(ApplicationError::from)?;
            let settled = executor
                .store
                .settle_tool_batch(
                    SettleToolBatchCommand {
                        node_instance_id: attempt.node_instance_id.clone(),
                        model_call_id,
                        fence: EffectAttemptFence {
                            invoking_node_attempt_id: attempt.attempt_id.clone(),
                            worker_id: attempt.worker_id.clone(),
                            lease_fence: attempt.lease_fence,
                            run_control_epoch: attempt.run_control_epoch,
                        },
                        checkpoint: result.checkpoint,
                    },
                    now,
                )
                .await
                .map_err(ApplicationError::from)?;
            Ok(MemoryDispatchResult::Settled(Box::new(settled)))
        }
    }
}
