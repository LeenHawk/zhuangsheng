use serde::{Deserialize, Serialize};

use super::{EffectAttemptFence, ir::LlmStreamEventIr};

pub struct PersistLlmStreamChunkCommand {
    pub node_instance_id: String,
    pub model_call_id: String,
    pub effect_attempt_id: String,
    pub chunk_no: u64,
    pub fence: EffectAttemptFence,
    pub events: Vec<LlmStreamEventIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLlmStreamChunk {
    pub durable_seq: u64,
    pub replayed: bool,
}
