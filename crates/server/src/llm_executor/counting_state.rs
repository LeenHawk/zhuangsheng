use serde_json::json;
use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    canonical,
    graph::LlmNodeExecutionSnapshot,
    llm::{
        ActiveCountEffectCheckpoint, CountExecutionPin, EffectAttemptFence, LlmLogicalCallStatus,
        LlmLoopCheckpoint, Operation, PreparedCountCall, RetryReadyResumeCountCall,
        adapter::WireGenerationRequest,
    },
    scheduler::ClaimedAttempt,
};

pub(super) struct CountRequestInput<'a> {
    pub attempt: &'a ClaimedAttempt,
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub transcript: &'a [zhuangsheng_core::llm::ir::LlmTurnItemIr],
    pub candidate_bytes: Vec<u8>,
    pub request_bytes: Vec<u8>,
    pub prior_checkpoint: Option<LlmLoopCheckpoint>,
    pub retry: Option<RetryReadyResumeCountCall>,
    pub provider_wire: Option<WireGenerationRequest>,
    pub credential: Option<&'a SecretValue>,
    pub now: i64,
}

pub(super) fn prepared_checkpoint(
    input: &CountRequestInput<'_>,
    prepared: &PreparedCountCall,
    effect_attempt_id: String,
) -> Result<LlmLoopCheckpoint, ApplicationError> {
    let mut checkpoint = input
        .prior_checkpoint
        .clone()
        .unwrap_or_else(|| initial_checkpoint(input));
    checkpoint.context_snapshot_ref = prepared.context_snapshot_ref.clone();
    checkpoint.transcript_ref = prepared.transcript_ref.clone();
    checkpoint.last_updated_by_attempt_id = input.attempt.attempt_id.clone();
    checkpoint.count_calls_used = checkpoint.count_calls_used.saturating_add(1);
    checkpoint.effect_watermark = effect_attempt_id;
    let pin = count_pin(input.execution)?;
    checkpoint.active_count_effect = Some(ActiveCountEffectCheckpoint {
        count_call_id: prepared.count_call_id.clone(),
        effect_id: prepared.effect_id.clone(),
        count_ordinal: checkpoint.count_calls_used,
        count_execution_pin_digest: pin.digest().map_err(|_| ApplicationError::Internal)?,
        trim_candidate_ref: prepared.trim_candidate_ref.clone(),
        trim_candidate_digest: canonical::hash_bytes(&input.candidate_bytes),
        request_digest: canonical::hash_bytes(&input.request_bytes),
        status: LlmLogicalCallStatus::Prepared,
        result_source: None,
        result_ref: None,
    });
    checkpoint.seal().map_err(|_| ApplicationError::Internal)
}

pub(super) fn initial_checkpoint(input: &CountRequestInput<'_>) -> LlmLoopCheckpoint {
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: input.attempt.node_instance_id.clone(),
        last_updated_by_attempt_id: input.attempt.attempt_id.clone(),
        graph_revision_id: input.execution.graph_revision_id.clone(),
        registry_snapshot: input.execution.tool_registry.clone(),
        context_snapshot_ref: String::new(),
        read_set_digest: input
            .attempt
            .context_snapshot
            .as_ref()
            .map(|snapshot| snapshot.read_set_digest.clone())
            .unwrap_or_default(),
        model_call_no: 0,
        transcript_ref: String::new(),
        continuation_ref: None,
        active_model_effect: None,
        active_count_effect: None,
        current_batch: Vec::new(),
        model_calls_used: 0,
        count_calls_used: 0,
        tool_calls_used: 0,
        effect_watermark: String::new(),
        wait_ids: Vec::new(),
        checksum: String::new(),
    }
}

pub(super) fn count_pin(
    execution: &LlmNodeExecutionSnapshot,
) -> Result<CountExecutionPin, ApplicationError> {
    let max_input = execution
        .limits
        .max_input_tokens
        .ok_or(ApplicationError::Internal)?;
    Ok(CountExecutionPin {
        generation_operation: execution.operation.clone(),
        provider_count_operation_key: execution
            .channel
            .spec
            .operation_keys
            .iter()
            .find(|key| key.operation == Operation::CountTokens)
            .copied(),
        local_counter_id: "gproxy_tokenize".into(),
        local_counter_version: 1,
        fallback_policy_version: 1,
        safety_margin_tokens: max_input.div_ceil(20).max(256),
    })
}

pub(super) fn reusable_completed_count<'a>(
    checkpoint: Option<&'a LlmLoopCheckpoint>,
    candidate_digest: &str,
    request_digest: &str,
) -> Option<&'a LlmLoopCheckpoint> {
    checkpoint.filter(|checkpoint| {
        checkpoint.count_calls_used > checkpoint.model_calls_used
            && checkpoint
                .active_count_effect
                .as_ref()
                .is_some_and(|active| {
                    active.status == LlmLogicalCallStatus::Completed
                        && active.trim_candidate_digest == candidate_digest
                        && active.request_digest == request_digest
                        && active.result_source.is_some()
                        && active.result_ref.is_some()
                })
    })
}

pub(super) fn estimate_tokens(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len())
        .unwrap_or(u64::MAX)
        .div_ceil(2)
        .max(1)
}

pub(super) fn fence(attempt: &ClaimedAttempt) -> EffectAttemptFence {
    EffectAttemptFence {
        invoking_node_attempt_id: attempt.attempt_id.clone(),
        worker_id: attempt.worker_id.clone(),
        lease_fence: attempt.lease_fence,
        run_control_epoch: attempt.run_control_epoch,
    }
}

pub(super) fn count_candidate_bytes(
    request: &zhuangsheng_core::llm::ir::LlmRequestIr,
) -> Result<Vec<u8>, ApplicationError> {
    canonical::to_vec(&json!({"schemaVersion":1,"request":request}))
        .map_err(|_| ApplicationError::Internal)
}
