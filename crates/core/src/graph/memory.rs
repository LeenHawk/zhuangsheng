use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterMemoryBinding {
    #[serde(default)]
    pub reads: Vec<RouterReadBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterReadBinding {
    pub id: String,
    #[serde(rename = "as")]
    pub alias: String,
    pub source: RouterReadSource,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub consistency: MemoryReadConsistency,
    pub limit: Option<u32>,
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouterReadSource {
    WorkingContext {
        scope: String,
        path: String,
    },
    LongTermMemory {
        scope: String,
        query: Option<MemoryQuery>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryQuery {
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub status: Option<MemoryRecordStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecordStatus {
    Active,
    Obsolete,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryReadConsistency {
    #[default]
    Snapshot,
    ValidateOnCommit,
}

fn default_true() -> bool {
    true
}

fn default_max_bytes() -> u64 {
    1024 * 1024
}
