use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{artifact::ArtifactRef, llm::OperationKey};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionIr {
    pub id: String,
    pub role: InstructionRole,
    #[serde(default)]
    pub content: Vec<LlmContentPartIr>,
    pub provenance: ContextProvenanceIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionRole {
    System,
    Developer,
    Policy,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProvenanceIr {
    pub id: String,
    pub item_id: String,
    pub source_type: String,
    pub source_id: String,
    pub trust: ContextTrust,
    pub sensitivity: ContextSensitivity,
    pub final_role: ProvenanceRole,
    #[serde(default)]
    pub transformations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextTrust {
    RuntimePolicy,
    TrustedConfig,
    UserInput,
    ExternalUntrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSensitivity {
    Public,
    Private,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceRole {
    Policy,
    System,
    Developer,
    Context,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LlmTurnItemIr {
    Message {
        id: String,
        role: MessageRole,
        content: Vec<LlmContentPartIr>,
        provenance: Option<ContextProvenanceIr>,
    },
    AssistantToolCall {
        id: String,
        call: ToolCallIr,
    },
    ToolResult {
        id: String,
        tool_call_id: String,
        tool_name: String,
        outcome: ToolResultOutcome,
        content: Vec<LlmContentPartIr>,
    },
    HostedTool {
        id: String,
        binding_id: String,
        kind: String,
        phase: HostedToolPhase,
        #[serde(default)]
        display_content: Vec<LlmContentPartIr>,
        opaque_item_ref: Option<OpaqueContinuationRef>,
    },
    Reasoning {
        id: String,
        summary: Option<String>,
        opaque_item_ref: Option<OpaqueContinuationRef>,
    },
}

impl LlmTurnItemIr {
    pub fn id(&self) -> &str {
        match self {
            Self::Message { id, .. }
            | Self::AssistantToolCall { id, .. }
            | Self::ToolResult { id, .. }
            | Self::HostedTool { id, .. }
            | Self::Reasoning { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallIr {
    pub id: String,
    pub provider_call_id: Option<String>,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultOutcome {
    Success,
    Error,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedToolPhase {
    Requested,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LlmContentPartIr {
    Text { text: String },
    Image { artifact_ref: ArtifactRef },
    File { artifact_ref: ArtifactRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalSensitiveEntryRef {
    pub object_id: String,
    pub entry_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpaqueContinuationRef {
    pub adapter_key: String,
    pub operation_key: OperationKey,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub model_call_id: String,
    #[serde(rename = "ref")]
    pub entry_ref: InternalSensitiveEntryRef,
    pub digest: String,
    pub expires_at: Option<i64>,
}
