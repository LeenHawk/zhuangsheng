use serde::{Deserialize, Serialize};

use super::{
    CountResultSource, EffectAttemptFence, EffectAttemptStatus, EffectRetryPolicy, EffectStatus,
    LlmLogicalCallStatus, LlmLoopCheckpoint, LlmOperationExecutionPin, OperationKey,
    ir::LlmTurnItemIr,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountExecutionPin {
    pub generation_operation: LlmOperationExecutionPin,
    pub provider_count_operation_key: Option<OperationKey>,
    pub local_counter_id: String,
    pub local_counter_version: u32,
    pub fallback_policy_version: u32,
    pub safety_margin_tokens: u64,
}

impl CountExecutionPin {
    pub fn digest(&self) -> crate::DomainResult<String> {
        crate::canonical::hash(self)
    }
}

pub struct PrepareCountCallCommand {
    pub count_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub count_ordinal: u64,
    pub channel_id: String,
    pub pin: CountExecutionPin,
    pub trim_candidate_bytes: Vec<u8>,
    pub request_bytes: Vec<u8>,
    pub effect_idempotency_key: String,
    pub retry_policy: EffectRetryPolicy,
    pub checkpoint: LlmLoopCheckpoint,
    pub initial_transcript: Option<Vec<LlmTurnItemIr>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCountCall {
    pub count_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub trim_candidate_ref: String,
    pub request_ref: String,
    pub context_snapshot_ref: String,
    pub transcript_ref: String,
    pub logical_status: LlmLogicalCallStatus,
    pub effect_status: EffectStatus,
    pub attempt_status: EffectAttemptStatus,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryReadyResumeCountCall {
    pub count_call_id: String,
    pub effect_id: String,
    pub trim_candidate_bytes: Vec<u8>,
    pub request_bytes: Vec<u8>,
}

pub struct StartCountCallCommand {
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub provider_request_id: Option<String>,
    pub checkpoint: LlmLoopCheckpoint,
}

pub enum CountCallOutcome {
    Completed {
        token_count: u64,
        source: CountResultSource,
    },
    Failed {
        error_bytes: Vec<u8>,
    },
    RetryReady {
        error_bytes: Vec<u8>,
    },
}

pub struct FinishCountCallCommand {
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub outcome: CountCallOutcome,
    pub checkpoint: LlmLoopCheckpoint,
}

pub struct PrepareCountCallRetryCommand {
    pub count_call_id: String,
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub checkpoint: LlmLoopCheckpoint,
}
