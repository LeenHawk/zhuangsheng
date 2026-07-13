use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    llm::{
        LlmChannelRevision, LlmNodeModelRef, LlmOperationExecutionPin, ModelCapabilityOverride,
        OperationKey,
        context::{ContextAssemblyConfig, ContextConfigSnapshot},
    },
    schema::JsonSchemaSpec,
};

use super::LlmMemoryBinding;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmNodeConfig {
    pub model: LlmNodeModelRef,
    #[serde(default)]
    pub capability_overrides: Vec<ModelCapabilityOverride>,
    pub context: ContextAssemblyConfig,
    pub memory: Option<LlmMemoryBinding>,
    #[serde(default)]
    pub tools: Vec<ToolGrant>,
    #[serde(default)]
    pub hosted_tools: Vec<HostedToolBinding>,
    pub request: Option<LlmRequestOptions>,
    pub output: Option<LlmOutputSpec>,
    pub streaming: Option<LlmNodeStreaming>,
    pub limits: Option<LlmNodeLimits>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequestOptions {
    pub generation: Option<GenerationOptionsIr>,
    pub extensions: Option<ProviderExtensionsIr>,
    pub tool_choice: Option<ToolChoiceIr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationOptionsIr {
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub temperature: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub top_p: Option<f64>,
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub stop: Vec<String>,
    pub seed: Option<i64>,
}

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<serde_json::Number>::deserialize(deserializer)?
        .map(|number| {
            number
                .as_f64()
                .ok_or_else(|| serde::de::Error::custom("number cannot be represented as f64"))
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolChoiceIr {
    Auto,
    None,
    Required,
    Named { name: String },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExtensionsIr {
    pub openai: Option<ProviderExtraIr>,
    pub claude: Option<ProviderExtraIr>,
    pub gemini: Option<ProviderExtraIr>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExtraIr {
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
    #[serde(default)]
    pub extra_body: BTreeMap<String, Value>,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LlmOutputSpec {
    Text {
        final_text: Option<LlmFinalText>,
        #[serde(default)]
        allow_empty: bool,
    },
    Json {
        schema: JsonSchemaSpec,
        #[serde(default)]
        strict: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmFinalText {
    LastAssistantTurn,
    AllAssistantText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmNodeStreaming {
    pub enabled: bool,
    pub audience: StreamingAudience,
    #[serde(default)]
    pub persist_chunks: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamingAudience {
    User,
    Trace,
    Both,
    Internal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmNodeLimits {
    pub max_model_calls: Option<u64>,
    pub max_count_calls: Option<u64>,
    pub max_tool_calls: Option<u64>,
    pub max_output_repairs: Option<u64>,
    pub max_concurrent_tools: Option<u64>,
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolGrant {
    pub binding_id: String,
    pub tool_id: String,
    pub version: String,
    pub exposed_name: Option<String>,
    #[serde(default)]
    pub scopes: Vec<ToolScopeGrant>,
    pub artifact: ArtifactGrant,
    #[serde(default)]
    pub constraints: BTreeMap<String, Value>,
    pub approval: Option<ToolApprovalPolicy>,
    pub failure_policy: Option<ToolFailurePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolScopeGrant {
    pub kind: ToolScopeKind,
    pub scope: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub origins: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolScopeKind {
    MemoryRead,
    MemoryProposal,
    StatePatch,
    ArtifactRead,
    ArtifactWrite,
    Network,
    LocalNetwork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactGrant {
    #[serde(default)]
    pub read_scopes: Vec<String>,
    #[serde(default)]
    pub write_scopes: Vec<String>,
    #[serde(default)]
    pub allowed_media_types: Vec<String>,
    pub max_objects: u64,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalPolicy {
    DescriptorDefault,
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFailurePolicy {
    pub invalid_call: ToolFailureAction,
    pub denied: ToolFailureAction,
    pub approval_required: ApprovalRequiredAction,
    pub execution_error: ToolFailureAction,
    pub max_attempts: u64,
    #[serde(default)]
    pub retry_backoff_ms: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureAction {
    ModelVisibleError,
    FailNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequiredAction {
    Wait,
    FailNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedToolBinding {
    pub binding_id: String,
    pub operation_key: OperationKey,
    pub hosted_kind: String,
    #[serde(default)]
    pub model_facing_config: BTreeMap<String, Value>,
    #[serde(default)]
    pub resource_scopes: Vec<String>,
    pub effect: ToolEffectSpec,
    pub max_uses_per_model_call: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEffectSpec {
    pub classification: EffectClassification,
    pub operation_key: String,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClassification {
    Pure,
    Idempotent,
    NonIdempotent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmNodeExecutionSnapshot {
    pub schema_version: u32,
    pub graph_revision_id: String,
    pub graph_content_hash: String,
    pub node_id: String,
    pub operation: LlmOperationExecutionPin,
    pub channel: LlmChannelRevision,
    pub context: ContextConfigSnapshot,
    pub capability_overrides: Vec<ModelCapabilityOverride>,
    pub memory: Option<LlmMemoryBinding>,
    pub tools: Vec<ToolGrant>,
    pub tool_registry: crate::llm::ToolRegistrySnapshot,
    pub tool_descriptors: Vec<crate::llm::ResolvedToolDescriptor>,
    pub hosted_tools: Vec<HostedToolBinding>,
    pub request: Option<LlmRequestOptions>,
    pub output: Option<LlmOutputSpec>,
    pub streaming: Option<LlmNodeStreaming>,
    pub limits: LlmNodeLimits,
}
