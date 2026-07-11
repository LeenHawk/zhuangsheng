use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateKind {
    WorkingContext,
    ArtifactMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    System,
    Node,
    Tool,
    Application,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorRef {
    pub kind: ActorKind,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JsonPatchOp {
    Add {
        path: String,
        value: Value,
    },
    Replace {
        path: String,
        value: Value,
    },
    Test {
        path: String,
        value: Value,
    },
    Remove {
        path: String,
    },
    Append {
        path: String,
        element_id: String,
        value: Value,
    },
}

impl JsonPatchOp {
    pub fn path(&self) -> &str {
        match self {
            Self::Add { path, .. }
            | Self::Replace { path, .. }
            | Self::Test { path, .. }
            | Self::Remove { path }
            | Self::Append { path, .. } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePatch {
    pub aggregate_kind: AggregateKind,
    pub aggregate_id: String,
    pub lineage_key: String,
    pub base_commit_id: String,
    pub operation_id: String,
    pub ops: Vec<JsonPatchOp>,
    pub schema_version: u32,
    pub policy_version: u32,
    pub author: ActorRef,
}
