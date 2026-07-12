use serde::{Deserialize, Serialize};

use crate::{
    application::memory::MemorySearchCommand,
    graph::{EffectClassification, MemoryToolGrant, ToolGrant},
};

use super::{
    EffectAttemptFence, EffectAttemptStatus, EffectRetryPolicy, EffectStatus, LlmLoopCheckpoint,
    ToolCallCheckpointStatus,
};

pub const TOOL_CALL_POLICY_VERSION: u32 = 1;
pub const MEMORY_SEARCH_TOOL_ID: &str = "builtin.search_memory";
pub const MEMORY_SEARCH_TOOL_VERSION: &str = "1";
pub const MEMORY_SEARCH_BINDING_ID: &str = "memory.search_memory";

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchToolCallDigestMaterial {
    pub query: MemorySearchCommand,
    pub grant: MemoryToolGrant,
    pub policy_version: u32,
}

impl MemorySearchToolCallDigestMaterial {
    pub fn digest(&self) -> crate::DomainResult<String> {
        crate::canonical::hash(self)
    }
}

pub struct ExecuteMemorySearchToolBatchCommand {
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub model_call_id: String,
    pub calls: Vec<MemorySearchToolCallCommand>,
    pub checkpoint: LlmLoopCheckpoint,
}

pub struct MemorySearchToolCallCommand {
    pub tool_call_id: String,
    pub provider_call_id: Option<String>,
    pub call_index: u64,
    pub call_digest: String,
    pub query: MemorySearchCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchToolCallView {
    pub tool_call_id: String,
    pub call_index: u64,
    pub query_ref: String,
    pub envelope_ref: String,
    pub output_ref: String,
    pub result_digest: String,
    pub scope_snapshot_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchToolRecord {
    pub memory_id: String,
    pub commit_id: String,
    pub content_hash: String,
    pub summary: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchToolEnvelope {
    pub records: Vec<MemorySearchToolRecord>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchToolBatchView {
    pub calls: Vec<MemorySearchToolCallView>,
    pub replayed: bool,
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

pub struct PrepareToolApprovalBatchCommand {
    pub wait_id: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub model_call_id: String,
    pub calls: Vec<PrepareToolApprovalCall>,
    pub checkpoint: LlmLoopCheckpoint,
}

pub struct PrepareToolApprovalCall {
    pub tool_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
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
    pub risk_summary: String,
    pub approval_expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedToolApprovalBatch {
    pub wait_id: String,
    pub tool_call_ids: Vec<String>,
    pub replayed: bool,
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

pub struct SettleToolBatchCommand {
    pub node_instance_id: String,
    pub model_call_id: String,
    pub fence: EffectAttemptFence,
    pub checkpoint: LlmLoopCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettledToolBatch {
    pub checkpoint: LlmLoopCheckpoint,
    pub transcript: Vec<crate::llm::ir::LlmTurnItemIr>,
}

pub struct LoadLlmResumeStateCommand {
    pub node_instance_id: String,
    pub fence: EffectAttemptFence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResumeState {
    pub checkpoint: LlmLoopCheckpoint,
    pub transcript: Vec<crate::llm::ir::LlmTurnItemIr>,
    pub output_repairs_used: u64,
    pub pending_output_repair: Option<super::PendingLlmOutputRepair>,
    pub retry_ready_model_call: Option<RetryReadyResumeModelCall>,
    pub retry_ready_count_call: Option<super::RetryReadyResumeCountCall>,
    pub prepared_tool_calls: Vec<PreparedResumeToolCall>,
    pub retry_ready_tool_calls: Vec<RetryReadyResumeToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryReadyResumeModelCall {
    pub model_call_id: String,
    pub effect_id: String,
    pub channel_id: String,
    pub operation: super::LlmOperationExecutionPin,
    pub request_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedResumeToolCall {
    pub tool_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub model_call_id: String,
    pub call_index: u64,
    pub binding_id: String,
    pub tool_id: String,
    pub tool_version: String,
    pub arguments: serde_json::Value,
    pub effect_idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryReadyResumeToolCall {
    pub tool_call_id: String,
    pub effect_id: String,
    pub model_call_id: String,
    pub call_index: u64,
    pub binding_id: String,
    pub tool_id: String,
    pub tool_version: String,
    pub arguments: serde_json::Value,
    pub effect_idempotency_key: String,
}

pub struct PrepareToolCallRetryCommand {
    pub tool_call_id: String,
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub checkpoint: LlmLoopCheckpoint,
}
