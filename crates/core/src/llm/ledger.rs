use serde::{Deserialize, Serialize};

use crate::{
    DomainResult, canonical,
    graph::EffectClassification,
    llm::ir::{LlmUsageIr, OpaqueContinuationRef},
};

use super::LlmOperationExecutionPin;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRegistryEntrySnapshot {
    pub tool_id: String,
    pub version: String,
    pub descriptor_digest: String,
    pub schema_compilation_digests: Vec<String>,
    pub implementation_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRegistrySnapshot {
    pub revision: String,
    pub entries: Vec<ToolRegistryEntrySnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmLogicalCallStatus {
    Prepared,
    Running,
    Completed,
    Failed,
    OutcomeUnknown,
    RetryReady,
    CancelledBeforeStart,
    AbandonedUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectStatus {
    Pending,
    Succeeded,
    Failed,
    OutcomeUnknown,
    CancelledBeforeStart,
    AbandonedUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectAttemptStatus {
    Prepared,
    Started,
    Succeeded,
    Failed,
    OutcomeUnknown,
    SupersededBeforeStart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveModelEffectCheckpoint {
    pub model_call_id: String,
    pub effect_id: String,
    pub status: LlmLogicalCallStatus,
    pub response_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountResultSource {
    Provider,
    Local,
    Estimate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveCountEffectCheckpoint {
    pub count_call_id: String,
    pub effect_id: String,
    pub count_ordinal: u64,
    pub count_execution_pin_digest: String,
    pub trim_candidate_ref: String,
    pub trim_candidate_digest: String,
    pub request_digest: String,
    pub status: LlmLogicalCallStatus,
    pub result_source: Option<CountResultSource>,
    pub result_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallCheckpointStatus {
    Requested,
    Validated,
    AwaitingApproval,
    Prepared,
    Running,
    Completed,
    Failed,
    Denied,
    OutcomeUnknown,
    RetryReady,
    CancelledBeforeStart,
    AbandonedUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallCheckpoint {
    pub tool_call_id: String,
    pub call_index: u64,
    pub call_digest: String,
    pub status: ToolCallCheckpointStatus,
    pub effect_id: Option<String>,
    pub output_ref: Option<String>,
    pub wait_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmLoopCheckpoint {
    pub schema_version: u32,
    pub node_instance_id: String,
    pub last_updated_by_attempt_id: String,
    pub graph_revision_id: String,
    pub registry_snapshot: ToolRegistrySnapshot,
    pub context_snapshot_ref: String,
    pub read_set_digest: String,
    pub model_call_no: u64,
    pub transcript_ref: String,
    pub continuation_ref: Option<OpaqueContinuationRef>,
    pub active_model_effect: Option<ActiveModelEffectCheckpoint>,
    pub active_count_effect: Option<ActiveCountEffectCheckpoint>,
    pub current_batch: Vec<ToolCallCheckpoint>,
    pub model_calls_used: u64,
    pub count_calls_used: u64,
    pub tool_calls_used: u64,
    pub effect_watermark: String,
    pub wait_ids: Vec<String>,
    pub checksum: String,
}

impl LlmLoopCheckpoint {
    pub fn seal(mut self) -> DomainResult<Self> {
        self.checksum = self.expected_checksum()?;
        Ok(self)
    }

    pub fn checksum_is_valid(&self) -> bool {
        self.expected_checksum()
            .is_ok_and(|expected| expected == self.checksum)
    }

    pub fn expected_checksum(&self) -> DomainResult<String> {
        let mut value = serde_json::to_value(self)
            .map_err(|error| crate::DomainError::Serialization(error.to_string()))?;
        value
            .as_object_mut()
            .expect("checkpoint serializes as object")
            .remove("checksum");
        canonical::hash(&value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectRetryPolicy {
    pub max_attempts: u32,
    pub backoff_ms: Vec<u64>,
}

pub struct PrepareModelCallCommand {
    pub model_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub call_no: u64,
    pub channel_id: String,
    pub operation: LlmOperationExecutionPin,
    pub request_bytes: Vec<u8>,
    pub effect_kind: String,
    pub effect_classification: EffectClassification,
    pub effect_operation_key: String,
    pub effect_idempotency_key: String,
    pub retry_policy: EffectRetryPolicy,
    pub checkpoint: LlmLoopCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedModelCall {
    pub model_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub model_status: LlmLogicalCallStatus,
    pub effect_status: EffectStatus,
    pub attempt_status: EffectAttemptStatus,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectAttemptFence {
    pub invoking_node_attempt_id: String,
    pub worker_id: String,
    pub lease_fence: u64,
    pub run_control_epoch: u64,
}

pub struct StartModelCallCommand {
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub provider_request_id: Option<String>,
    pub checkpoint: LlmLoopCheckpoint,
}

pub enum ModelCallEffectOutcome {
    Completed {
        response_bytes: Vec<u8>,
        usage: Option<LlmUsageIr>,
    },
    Failed {
        error_bytes: Vec<u8>,
    },
    OutcomeUnknown {
        error_bytes: Vec<u8>,
    },
    RetryReady {
        error_bytes: Vec<u8>,
    },
}

pub struct FinishModelCallCommand {
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub outcome: ModelCallEffectOutcome,
    pub checkpoint: LlmLoopCheckpoint,
}

pub struct PrepareModelCallRetryCommand {
    pub model_call_id: String,
    pub effect_attempt_id: String,
    pub fence: EffectAttemptFence,
    pub checkpoint: LlmLoopCheckpoint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_checksum_covers_semantic_state() {
        let checkpoint = LlmLoopCheckpoint {
            schema_version: 1,
            node_instance_id: "instance-1".into(),
            last_updated_by_attempt_id: "attempt-1".into(),
            graph_revision_id: "revision-1".into(),
            registry_snapshot: ToolRegistrySnapshot {
                revision: "registry-1".into(),
                entries: Vec::new(),
            },
            context_snapshot_ref: "object-context".into(),
            read_set_digest: "sha256:read".into(),
            model_call_no: 1,
            transcript_ref: "object-transcript".into(),
            continuation_ref: None,
            active_model_effect: Some(ActiveModelEffectCheckpoint {
                model_call_id: "model-call-1".into(),
                effect_id: "effect-1".into(),
                status: LlmLogicalCallStatus::Prepared,
                response_ref: None,
            }),
            active_count_effect: None,
            current_batch: Vec::new(),
            model_calls_used: 1,
            count_calls_used: 0,
            tool_calls_used: 0,
            effect_watermark: "attempt-1".into(),
            wait_ids: Vec::new(),
            checksum: String::new(),
        }
        .seal()
        .unwrap();
        assert!(checkpoint.checksum_is_valid());
        let mut corrupted = checkpoint;
        corrupted.model_calls_used = 2;
        assert!(!corrupted.checksum_is_valid());
    }
}
