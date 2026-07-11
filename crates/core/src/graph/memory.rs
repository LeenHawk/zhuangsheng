use serde::{Deserialize, Serialize};

use super::InputSelector;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMemoryBinding {
    #[serde(default)]
    pub reads: Vec<StaticMemoryRead>,
    #[serde(default)]
    pub working_writes: Vec<StaticContextWrite>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmMemoryBinding {
    #[serde(flatten)]
    pub node: NodeMemoryBinding,
    #[serde(default)]
    pub tools: Vec<MemoryToolGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticMemoryRead {
    pub id: String,
    #[serde(rename = "as")]
    pub alias: String,
    pub source: StaticMemoryReadSource,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub consistency: MemoryReadConsistency,
    pub limit: Option<u32>,
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StaticMemoryReadSource {
    WorkingContext {
        scope: String,
        path: String,
    },
    LongTermMemory {
        scope: String,
        query: Option<MemoryQuery>,
    },
    Artifact {
        scope: String,
        artifact_ref_from: PreExecutionValueSelector,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreExecutionValueSelector {
    pub source: PreExecutionValueSource,
    pub source_name: String,
    pub selector: InputSelector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreExecutionValueSource {
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticContextWrite {
    pub scope: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryToolGrant {
    pub capability: MemoryToolCapability,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub max_results: Option<u32>,
    pub max_proposal_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryToolCapability {
    SearchMemory,
    ProposeMemoryChange,
}

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
