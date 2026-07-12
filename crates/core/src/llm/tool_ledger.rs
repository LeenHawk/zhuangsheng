use serde::{Deserialize, Serialize};

use crate::graph::{EffectClassification, ToolGrant};

use super::{
    EffectAttemptFence, EffectAttemptStatus, EffectRetryPolicy, EffectStatus, LlmLoopCheckpoint,
    ToolCallCheckpointStatus,
};

pub const TOOL_CALL_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallDigestMaterial {
    pub binding_id: String,
    pub tool_id: String,
    pub tool_version: String,
    pub arguments: serde_json::Value,
    pub grant: ToolGrant,
    pub descriptor_digest: String,
    pub schema_compilation_digests: Vec<String>,
    pub implementation_digest: String,
    pub policy_version: u32,
}

impl ToolCallDigestMaterial {
    pub fn digest(&self) -> crate::DomainResult<String> {
        crate::canonical::hash(self)
    }
}

pub struct PrepareToolCallCommand {
    pub tool_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub model_call_id: String,
    pub provider_call_id: Option<String>,
    pub call_index: u64,
    pub binding_id: String,
    pub tool_id: String,
    pub tool_version: String,
    pub call_digest: String,
    pub arguments_bytes: Vec<u8>,
    pub descriptor_digest: String,
    pub schema_compilation_digests: Vec<String>,
    pub implementation_digest: String,
    pub effect_classification: EffectClassification,
    pub effect_operation_key: String,
    pub descriptor_requires_approval: bool,
    pub effect_idempotency_key: String,
    pub retry_policy: EffectRetryPolicy,
    pub checkpoint: LlmLoopCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedToolCall {
    pub tool_call_id: String,
    pub effect_id: Option<String>,
    pub effect_attempt_id: Option<String>,
    pub arguments_ref: String,
    pub status: ToolCallCheckpointStatus,
    pub effect_status: Option<EffectStatus>,
    pub attempt_status: Option<EffectAttemptStatus>,
    pub replayed: bool,
}

pub struct StartToolCallCommand {
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub provider_request_id: Option<String>,
    pub checkpoint: LlmLoopCheckpoint,
}

pub enum ToolCallOutcome {
    Completed { output_bytes: Vec<u8> },
    Failed { error_bytes: Vec<u8> },
    OutcomeUnknown { error_bytes: Vec<u8> },
    RetryReady { error_bytes: Vec<u8> },
}

pub struct FinishToolCallCommand {
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub outcome: ToolCallOutcome,
    pub checkpoint: LlmLoopCheckpoint,
}

pub struct PrepareToolCallRetryCommand {
    pub tool_call_id: String,
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub checkpoint: LlmLoopCheckpoint,
}
