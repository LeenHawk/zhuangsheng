use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    graph::LlmNodeExecutionSnapshot,
    llm::{
        EffectAttemptFence, LlmLogicalCallStatus, LlmLoopCheckpoint, LlmRequestBuildOutput,
        PrepareModelCallRetryCommand, RetryReadyResumeModelCall,
        adapter::{AdapterExecutionOptions, encode_generation_request},
    },
    scheduler::ClaimedAttempt,
};

use crate::llm_executor_support::{finalize_failure, new_id, set_model_status};

use super::{
    LocalLlmExecutor,
    model_call::ModelCallResult,
    model_request::durable_generation_request,
    model_transport::{PreparedModelCallInput, execute_prepared_model_call},
    opaque_resources::resolve_opaque_resources,
};

pub(super) struct RetryModelCallInput<'a> {
    pub attempt: &'a ClaimedAttempt,
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub resume: RetryReadyResumeModelCall,
    pub checkpoint: LlmLoopCheckpoint,
    pub built: LlmRequestBuildOutput,
    pub credential: Option<&'a SecretValue>,
    pub reserved_output: u64,
    pub now: i64,
}

pub(super) async fn retry_model_call(
    executor: &LocalLlmExecutor,
    input: RetryModelCallInput<'_>,
) -> Result<ModelCallResult, ApplicationError> {
    let RetryModelCallInput {
        attempt,
        execution,
        resume,
        mut checkpoint,
        built,
        credential,
        reserved_output,
        now,
    } = input;
    if resume.channel_id != execution.channel.channel_id
        || resume.operation != execution.operation
        || execution.channel.id != resume.operation.channel_revision_id
    {
        return Ok(ModelCallResult::Terminal(finalize_failure(
            "llm_model_retry_pin_mismatch",
            "persisted model call does not match the execution snapshot",
        )));
    }
    let streaming = execution
        .streaming
        .as_ref()
        .is_some_and(|streaming| streaming.enabled);
    let resources =
        resolve_opaque_resources(executor, &execution.operation, &built.request, now).await?;
    let options = AdapterExecutionOptions {
        stream: streaming,
        max_output_tokens: reserved_output,
    };
    let wire =
        match encode_generation_request(&resume.operation, &built.request, &resources, options) {
            Ok(wire) => wire,
            Err(error) => {
                return Ok(ModelCallResult::Terminal(finalize_failure(
                    error.code,
                    &error.message,
                )));
            }
        };
    let durable_request = durable_generation_request(&resume.operation, &built.request, options)?;
    if durable_request != resume.request_bytes {
        return Ok(ModelCallResult::Terminal(finalize_failure(
            "llm_model_retry_request_mismatch",
            "persisted model request cannot be reproduced from the durable transcript",
        )));
    }
    let active = checkpoint
        .active_model_effect
        .as_ref()
        .ok_or(ApplicationError::Internal)?;
    if active.model_call_id != resume.model_call_id
        || active.effect_id != resume.effect_id
        || active.status != LlmLogicalCallStatus::RetryReady
        || active.response_ref.is_some()
        || !checkpoint.current_batch.is_empty()
    {
        return Err(ApplicationError::Internal);
    }
    let effect_attempt_id = new_id("effectattempt");
    checkpoint.last_updated_by_attempt_id = attempt.attempt_id.clone();
    checkpoint.effect_watermark = effect_attempt_id.clone();
    set_model_status(&mut checkpoint, LlmLogicalCallStatus::Prepared);
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    executor
        .store
        .prepare_model_call_retry(
            PrepareModelCallRetryCommand {
                model_call_id: resume.model_call_id.clone(),
                effect_attempt_id: effect_attempt_id.clone(),
                fence: EffectAttemptFence {
                    invoking_node_attempt_id: attempt.attempt_id.clone(),
                    worker_id: attempt.worker_id.clone(),
                    lease_fence: attempt.lease_fence,
                    run_control_epoch: attempt.run_control_epoch,
                },
                checkpoint: checkpoint.clone(),
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    execute_prepared_model_call(
        executor,
        PreparedModelCallInput {
            attempt,
            execution,
            built,
            wire,
            model_call_id: resume.model_call_id,
            effect_attempt_id,
            checkpoint,
            credential,
            now,
        },
    )
    .await
}
