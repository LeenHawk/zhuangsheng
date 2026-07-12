use serde_json::json;
use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    canonical,
    graph::LlmNodeExecutionSnapshot,
    llm::{
        CountCallOutcome, CountResultSource, FinishCountCallCommand, LlmLogicalCallStatus,
        LlmLoopCheckpoint, Operation, StartCountCallCommand,
        adapter::{WireGenerationRequest, decode_count_terminal, encode_count_request},
        ir::LlmRequestIr,
    },
    scheduler::ClaimedAttempt,
};

use super::{LocalLlmExecutor, counting_state::fence};

pub(super) fn provider_count_wire(
    execution: &LlmNodeExecutionSnapshot,
    generation: &WireGenerationRequest,
) -> Option<WireGenerationRequest> {
    let operation = execution
        .channel
        .spec
        .operation_keys
        .iter()
        .find(|key| key.operation == Operation::CountTokens)
        .copied()?;
    encode_count_request(generation, operation).ok()
}

pub(super) fn durable_count_request(
    request: &LlmRequestIr,
    wire: Option<&WireGenerationRequest>,
) -> Result<Vec<u8>, ApplicationError> {
    canonical::to_vec(&json!({
        "schemaVersion":1,
        "kind":"count_request_receipt",
        "providerOperation":wire.map(|wire| &wire.operation),
        "providerWireDigest":wire.map(|wire| canonical::hash_bytes(wire.body())),
        "request":request,
    }))
    .map_err(|_| ApplicationError::Internal)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_count(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    execution: &LlmNodeExecutionSnapshot,
    effect_attempt_id: String,
    mut checkpoint: LlmLoopCheckpoint,
    provider_wire: Option<WireGenerationRequest>,
    credential: Option<&SecretValue>,
    estimate: u64,
    now: i64,
) -> Result<LlmLoopCheckpoint, ApplicationError> {
    let (token_count, source) = if let Some(wire) = provider_wire {
        checkpoint
            .active_count_effect
            .as_mut()
            .ok_or(ApplicationError::Internal)?
            .status = LlmLogicalCallStatus::Running;
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        executor
            .store
            .start_count_call(
                StartCountCallCommand {
                    effect_attempt_id: effect_attempt_id.clone(),
                    fence: fence(attempt),
                    provider_request_id: None,
                    checkpoint: checkpoint.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        match executor
            .provider
            .send(&execution.channel, &wire, credential)
            .await
            .ok()
            .and_then(|response| decode_count_terminal(&wire, &response.body).ok())
        {
            Some(count) => (count, CountResultSource::Provider),
            None => (estimate, CountResultSource::Estimate),
        }
    } else {
        (estimate, CountResultSource::Estimate)
    };
    checkpoint
        .active_count_effect
        .as_mut()
        .ok_or(ApplicationError::Internal)?
        .status = LlmLogicalCallStatus::Completed;
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    let completed = executor
        .store
        .finish_count_call(
            FinishCountCallCommand {
                effect_attempt_id,
                fence: fence(attempt),
                outcome: CountCallOutcome::Completed {
                    token_count,
                    source,
                },
                checkpoint,
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    #[cfg(test)]
    if let Some(pause) = &executor.count_completed_pause {
        pause.wait_once().await;
    }
    Ok(completed)
}
