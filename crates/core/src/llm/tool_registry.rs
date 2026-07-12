use serde::{Deserialize, Serialize};

use crate::{
    DomainResult, canonical,
    graph::{ToolEffectSpec, ToolScopeKind},
    schema::JsonSchemaSpec,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub tool_id: String,
    pub version: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema: JsonSchemaSpec,
    pub binding_config_schema: Option<JsonSchemaSpec>,
    pub effect: ToolEffectSpec,
    pub supports_parallel: bool,
    #[serde(default)]
    pub required_scopes: Vec<ToolScopeRequirement>,
    pub limits: ToolLimits,
}

impl ToolDescriptor {
    pub fn digest(&self) -> DomainResult<String> {
        canonical::hash(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolScopeRequirement {
    pub kind: ToolScopeKind,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLimits {
    pub timeout_ms: u64,
    pub max_input_bytes: u64,
    pub max_llm_result_bytes: u64,
    pub max_artifact_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedToolDescriptor {
    pub descriptor: ToolDescriptor,
    pub descriptor_digest: String,
    #[serde(default)]
    pub schema_compilation_digests: Vec<String>,
    pub implementation_digest: String,
    pub executor_key: String,
}
