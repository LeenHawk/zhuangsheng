use gproxy_protocol::OperationKey;
use serde::{Deserialize, Serialize};

use super::{LlmConfigResult, SecretRef};

pub const MODEL_CAPABILITY_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelTransportPolicy {
    #[serde(default)]
    pub allow_loopback_http: bool,
    #[serde(default)]
    pub allow_unauthenticated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChannelCredential {
    Secret { api_key_ref: SecretRef },
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapabilityName {
    Streaming,
    ToolCalling,
    StructuredOutput,
    VisionInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilityOverride {
    pub feature: ModelCapabilityName,
    pub assumption: ModelCapabilityAssumption,
    pub reason: String,
    pub acknowledgement_ref: String,
    pub policy_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapabilityAssumption {
    Supported,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    pub streaming: Option<bool>,
    pub tool_calling: Option<bool>,
    pub structured_output: Option<bool>,
    pub vision_input: Option<bool>,
}

impl ModelCapabilities {
    pub fn get(&self, feature: ModelCapabilityName) -> Option<bool> {
        match feature {
            ModelCapabilityName::Streaming => self.streaming,
            ModelCapabilityName::ToolCalling => self.tool_calling,
            ModelCapabilityName::StructuredOutput => self.structured_output,
            ModelCapabilityName::VisionInput => self.vision_input,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogPolicy {
    Open,
    Allowlist,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelModelCatalog {
    pub operation_key: OperationKey,
    pub policy: ModelCatalogPolicy,
    #[serde(default)]
    pub models: Vec<ChannelModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ChannelCapability {
    HostedTool {
        operation_key: OperationKey,
        hosted_kind: String,
    },
    ToolBasedCompact {
        operation_key: OperationKey,
        hosted_kind: String,
    },
}

impl ChannelCapability {
    pub(crate) fn operation_key(&self) -> OperationKey {
        match self {
            Self::HostedTool { operation_key, .. }
            | Self::ToolBasedCompact { operation_key, .. } => *operation_key,
        }
    }

    pub(crate) fn hosted_kind_mut(&mut self) -> &mut String {
        match self {
            Self::HostedTool { hosted_kind, .. } | Self::ToolBasedCompact { hosted_kind, .. } => {
                hosted_kind
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmChannelRevisionSpec {
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub base_url: String,
    pub transport_policy: ChannelTransportPolicy,
    pub credential: ChannelCredential,
    #[serde(default)]
    pub operation_keys: Vec<OperationKey>,
    #[serde(default)]
    pub model_catalogs: Vec<ChannelModelCatalog>,
    #[serde(default)]
    pub capabilities: Vec<ChannelCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmChannelRevision {
    pub id: String,
    pub channel_id: String,
    pub revision_no: u64,
    #[serde(flatten)]
    pub spec: LlmChannelRevisionSpec,
    pub content_hash: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmNodeModelRef {
    pub channel_id: String,
    pub model_id: String,
    pub model_name: Option<String>,
    pub operation_key: OperationKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmOperationExecutionPin {
    pub channel_revision_id: String,
    pub model_id: String,
    pub operation_key: OperationKey,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelCapabilityRequirements {
    pub streaming: bool,
    pub tool_calling: bool,
    pub structured_output: bool,
    pub vision_input: bool,
}

impl ModelCapabilityRequirements {
    pub(crate) fn required(&self) -> impl Iterator<Item = ModelCapabilityName> {
        [
            (self.streaming, ModelCapabilityName::Streaming),
            (self.tool_calling, ModelCapabilityName::ToolCalling),
            (
                self.structured_output,
                ModelCapabilityName::StructuredOutput,
            ),
            (self.vision_input, ModelCapabilityName::VisionInput),
        ]
        .into_iter()
        .filter_map(|(required, feature)| required.then_some(feature))
    }
}

pub fn revision_content_hash(spec: &LlmChannelRevisionSpec) -> LlmConfigResult<String> {
    crate::canonical::hash(spec)
        .map_err(|error| super::LlmConfigError::new("invalid_channel_revision", error.to_string()))
}
