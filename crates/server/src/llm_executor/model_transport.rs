use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    graph::LlmNodeExecutionSnapshot,
    llm::{
        EffectAttemptFence, FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint,
        LlmRequestBuildOutput, ModelCallEffectOutcome, StartModelCallCommand,
        adapter::{WireGenerationRequest, decode_generation_terminal},
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::{finalize_failure, provider_error_bytes, set_model_status};

use super::{
    LocalLlmExecutor,
    model_call::ModelCallResult,
    model_completion::{CompletedResponseInput, finish_decoded_model_call},
    model_stream::execute_stream_model_call,
};

pub(super) struct PreparedModelCallInput<'a> {
    pub attempt: &'a ClaimedAttempt,
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub built: LlmRequestBuildOutput,
    pub wire: WireGenerationRequest,
    pub model_call_id: String,
    pub effect_attempt_id: String,
    pub checkpoint: LlmLoopCheckpoint,
    pub credential: Option<&'a SecretValue>,
    pub now: i64,
}

pub(super) async fn execute_prepared_model_call(
    executor: &LocalLlmExecutor,
    input: PreparedModelCallInput<'_>,
) -> Result<ModelCallResult, ApplicationError> {
    let PreparedModelCallInput {
        attempt,
        execution,
        built,
        wire,
        model_call_id,
        effect_attempt_id,
        mut checkpoint,
        credential,
        now,
    } = input;
    let fence = fence(attempt);
    set_model_status(&mut checkpoint, LlmLogicalCallStatus::Running);
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    executor
        .store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: effect_attempt_id.clone(),
                fence: fence.clone(),
                provider_request_id: None,
                checkpoint: checkpoint.clone(),
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    if execution
        .streaming
        .as_ref()
        .is_some_and(|streaming| streaming.enabled)
    {
        return execute_stream_model_call(
            executor,
            super::model_stream::StreamModelCallInput {
                attempt,
                execution,
                built,
                wire,
                model_call_id,
                effect_attempt_id,
                checkpoint,
                fence,
                credential,
                now,
            },
        )
        .await;
    }
    let response = match executor
        .provider
        .send(&execution.channel, &wire, credential)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let status = if error.outcome_unknown {
                LlmLogicalCallStatus::OutcomeUnknown
            } else {
                LlmLogicalCallStatus::Failed
            };
            set_model_status(&mut checkpoint, status);
            checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
            let error_bytes = provider_error_bytes(&error);
            executor
                .store
                .finish_model_call(
                    FinishModelCallCommand {
                        effect_attempt_id,
                        fence,
                        outcome: if error.outcome_unknown {
                            ModelCallEffectOutcome::OutcomeUnknown { error_bytes }
                        } else {
                            ModelCallEffectOutcome::Failed { error_bytes }
                        },
                        checkpoint,
                        transcript: None,
                    },
                    now,
                )
                .await
                .map_err(ApplicationError::from)?;
            return Ok(ModelCallResult::Terminal(if error.outcome_unknown {
                LlmAttemptExecution::Handled
            } else {
                finalize_failure(error.code, &error.safe_message)
            }));
        }
    };
    let decoded = decode_generation_terminal(&execution.operation, &model_call_id, &response.body);
    finish_decoded_model_call(
        executor,
        CompletedResponseInput {
            built,
            model_call_id,
            effect_attempt_id,
            checkpoint,
            fence,
            response_bytes: response.body,
            decoded,
            now,
        },
    )
    .await
}

fn fence(attempt: &ClaimedAttempt) -> EffectAttemptFence {
    EffectAttemptFence {
        invoking_node_attempt_id: attempt.attempt_id.clone(),
        worker_id: attempt.worker_id.clone(),
        lease_fence: attempt.lease_fence,
        run_control_epoch: attempt.run_control_epoch,
    }
}
