use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    graph::{GenerationOptionsIr, ProviderExtensionsIr, ToolChoiceIr},
    schema::JsonSchemaSpec,
};

use super::{InstructionIr, LlmTurnItemIr, OpaqueContinuationRef};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmRequestIr {
    pub model: String,
    #[serde(default)]
    pub instructions: Vec<InstructionIr>,
    #[serde(default)]
    pub transcript: Vec<LlmTurnItemIr>,
    #[serde(default)]
    pub tools: Vec<ToolDescriptorIr>,
    #[serde(default)]
    pub hosted_tools: Vec<HostedToolDescriptorIr>,
    pub tool_choice: Option<ToolChoiceIr>,
    pub response_format: Option<ResponseFormatIr>,
    pub generation: Option<GenerationOptionsIr>,
    pub extensions: Option<ProviderExtensionsIr>,
    #[serde(default)]
    pub metadata: BTreeMap<String, MetadataValue>,
    pub continuation: Option<OpaqueContinuationRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptorIr {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: JsonSchemaSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedToolDescriptorIr {
    pub binding_id: String,
    pub hosted_kind: String,
    #[serde(default)]
    pub config: BTreeMap<String, MetadataValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ResponseFormatIr {
    Text,
    Json {
        schema: Option<JsonSchemaSpec>,
        #[serde(default)]
        strict: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetadataValue {
    Null,
    Boolean(bool),
    Number(serde_json::Number),
    String(String),
}
