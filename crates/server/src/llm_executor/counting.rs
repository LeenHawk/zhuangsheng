use zhuangsheng_core::{
    application::ApplicationError,
    canonical,
    llm::{
        ActiveCountEffectCheckpoint, EffectRetryPolicy, LlmLogicalCallStatus, LlmLoopCheckpoint,
        PrepareCountCallCommand, PrepareCountCallRetryCommand, RetryReadyResumeCountCall,
    },
};

use crate::llm_executor_support::new_id;

use super::{
    LocalLlmExecutor,
    counting_provider::execute_count,
    counting_state::{
        CountRequestInput, count_pin, estimate_tokens, fence, initial_checkpoint,
        prepared_checkpoint, reusable_completed_count,
    },
};

pub(super) async fn count_request(
    executor: &LocalLlmExecutor,
    mut input: CountRequestInput<'_>,
) -> Result<LlmLoopCheckpoint, ApplicationError> {
    if input
        .prior_checkpoint
        .as_ref()
        .is_some_and(|checkpoint| checkpoint.model_calls_used > 0)
    {
        if input.retry.is_some() {
            return Err(ApplicationError::Internal);
        }
        return input.prior_checkpoint.ok_or(ApplicationError::Internal);
    }
    let candidate_digest = canonical::hash_bytes(&input.candidate_bytes);
    let request_digest = canonical::hash_bytes(&input.request_bytes);
    if let Some(checkpoint) = reusable_completed_count(
        input.prior_checkpoint.as_ref(),
        &candidate_digest,
        &request_digest,
    ) {
        if input.retry.is_some() {
            return Err(ApplicationError::Internal);
        }
        return Ok(checkpoint.clone());
    }
    if let Some(retry) = input.retry.take() {
        return retry_count(executor, input, retry, candidate_digest, request_digest).await;
    }
    prepare_count(executor, input, candidate_digest, request_digest).await
}

async fn prepare_count(
    executor: &LocalLlmExecutor,
    input: CountRequestInput<'_>,
    candidate_digest: String,
    request_digest: String,
) -> Result<LlmLoopCheckpoint, ApplicationError> {
    let pin = count_pin(input.execution)?;
    let count_call_id = new_id("countcall");
    let effect_id = new_id("effect");
    let effect_attempt_id = new_id("effectattempt");
    let ordinal = input.prior_checkpoint.as_ref().map_or(1, |checkpoint| {
        checkpoint.count_calls_used.saturating_add(1)
    });
    let initial = input.prior_checkpoint.is_none();
    let mut checkpoint = input
        .prior_checkpoint
        .clone()
        .unwrap_or_else(|| initial_checkpoint(&input));
    checkpoint.last_updated_by_attempt_id = input.attempt.attempt_id.clone();
    checkpoint.count_calls_used = ordinal;
    checkpoint.effect_watermark = effect_attempt_id.clone();
    checkpoint.active_count_effect = Some(ActiveCountEffectCheckpoint {
        count_call_id: count_call_id.clone(),
        effect_id: effect_id.clone(),
        count_ordinal: ordinal,
        count_execution_pin_digest: pin.digest().map_err(|_| ApplicationError::Internal)?,
        trim_candidate_ref: String::new(),
        trim_candidate_digest: candidate_digest,
        request_digest,
        status: LlmLogicalCallStatus::Prepared,
        result_source: None,
        result_ref: None,
    });
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    let prepared = executor
        .store
        .prepare_count_call(
            PrepareCountCallCommand {
                count_call_id,
                effect_id,
                effect_attempt_id: effect_attempt_id.clone(),
                node_instance_id: input.attempt.node_instance_id.clone(),
                originating_attempt_id: input.attempt.attempt_id.clone(),
                count_ordinal: ordinal,
                channel_id: input.execution.channel.channel_id.clone(),
                pin,
                trim_candidate_bytes: input.candidate_bytes.clone(),
                request_bytes: input.request_bytes.clone(),
                effect_idempotency_key: format!(
                    "count:{}:{ordinal}",
                    input.attempt.node_instance_id
                ),
                retry_policy: EffectRetryPolicy {
                    max_attempts: 3,
                    backoff_ms: vec![50, 250],
                },
                checkpoint,
                initial_transcript: initial.then(|| input.transcript.to_vec()),
            },
            input.now,
        )
        .await
        .map_err(ApplicationError::from)?;
    let checkpoint = prepared_checkpoint(&input, &prepared, effect_attempt_id.clone())?;
    #[cfg(test)]
    if let Some(pause) = &executor.count_prepared_pause {
        pause.wait_once().await;
    }
    execute_count(
        executor,
        input.attempt,
        input.execution,
        effect_attempt_id,
        checkpoint,
        input.provider_wire,
        input.credential,
        estimate_tokens(&input.request_bytes),
        input.now,
    )
    .await
}

async fn retry_count(
    executor: &LocalLlmExecutor,
    input: CountRequestInput<'_>,
    retry: RetryReadyResumeCountCall,
    candidate_digest: String,
    request_digest: String,
) -> Result<LlmLoopCheckpoint, ApplicationError> {
    if retry.trim_candidate_bytes != input.candidate_bytes
        || retry.request_bytes != input.request_bytes
    {
        return Err(ApplicationError::Internal);
    }
    let mut checkpoint = input.prior_checkpoint.ok_or(ApplicationError::Internal)?;
    let active = checkpoint
        .active_count_effect
        .as_mut()
        .ok_or(ApplicationError::Internal)?;
    if active.count_call_id != retry.count_call_id
        || active.effect_id != retry.effect_id
        || active.trim_candidate_digest != candidate_digest
        || active.request_digest != request_digest
        || active.status != LlmLogicalCallStatus::RetryReady
    {
        return Err(ApplicationError::Internal);
    }
    let effect_attempt_id = new_id("effectattempt");
    active.status = LlmLogicalCallStatus::Prepared;
    checkpoint.last_updated_by_attempt_id = input.attempt.attempt_id.clone();
    checkpoint.effect_watermark = effect_attempt_id.clone();
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    executor
        .store
        .prepare_count_call_retry(
            PrepareCountCallRetryCommand {
                count_call_id: retry.count_call_id,
                effect_attempt_id: effect_attempt_id.clone(),
                fence: fence(input.attempt),
                checkpoint: checkpoint.clone(),
            },
            input.now,
        )
        .await
        .map_err(ApplicationError::from)?;
    execute_count(
        executor,
        input.attempt,
        input.execution,
        effect_attempt_id,
        checkpoint,
        input.provider_wire,
        input.credential,
        estimate_tokens(&input.request_bytes),
        input.now,
    )
    .await
}
