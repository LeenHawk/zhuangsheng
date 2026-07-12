use serde::{Deserialize, Serialize};

use super::{EffectAttemptFence, LlmLoopCheckpoint, ir::LlmTurnItemIr};

pub struct PrepareLlmOutputRepairCommand {
    pub repair_id: String,
    pub node_instance_id: String,
    pub source_model_call_id: String,
    pub extracted_bytes_digest: String,
    pub error_code: String,
    pub instruction: LlmTurnItemIr,
    pub fence: EffectAttemptFence,
    pub checkpoint: LlmLoopCheckpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedLlmOutputRepair {
    pub repair_id: String,
    pub repair_no: u64,
    pub checkpoint: LlmLoopCheckpoint,
    pub transcript: Vec<LlmTurnItemIr>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingLlmOutputRepair {
    pub repair_id: String,
    pub repair_no: u64,
    pub source_model_call_id: String,
    pub extracted_bytes_digest: String,
    pub error_code: String,
}
