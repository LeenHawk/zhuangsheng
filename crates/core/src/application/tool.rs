use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    graph::ToolGrant,
    llm::{ResolvedToolDescriptor, ToolDescriptor, ir::LlmContentPartIr},
    schema::JsonSchemaSpec,
};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptorView {
    pub tool_id: String,
    pub version: String,
    pub name: String,
    pub description: Option<String>,
    pub input_schema: JsonSchemaSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredToolView {
    pub resolved: ResolvedToolDescriptor,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishToolCommand {
    pub descriptor: ToolDescriptor,
    pub implementation_digest: String,
    pub executor_key: String,
    pub enabled: bool,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetToolEnabledCommand {
    pub tool_id: String,
    pub version: String,
    pub enabled: bool,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    pub run_id: String,
    pub node_instance_id: String,
    pub tool_call_id: String,
    pub binding_id: String,
    pub tool_id: String,
    pub tool_version: String,
    pub arguments: serde_json::Value,
    pub effect_idempotency_key: String,
    pub grant: ToolGrant,
    pub descriptor: ResolvedToolDescriptor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionContext {
    pub invocation: ToolInvocation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallOutput {
    pub parts: Vec<ToolOutputPart>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ToolOutputPart {
    LlmResult { content: Vec<LlmContentPartIr> },
    Artifact { staging_id: String },
    StatePatch { patch: serde_json::Value },
    MemoryChangeProposal { proposal: serde_json::Value },
    UserMessage { content: Vec<LlmContentPartIr> },
    Evidence { refs: Vec<String> },
    Debug { summary: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionError {
    pub code: String,
    pub safe_message: String,
    pub retryable: bool,
    pub outcome_unknown: bool,
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        context: ToolExecutionContext,
    ) -> Result<ToolCallOutput, ToolExecutionError>;
}

#[async_trait]
pub trait ToolRegistryService: Send + Sync {
    async fn publish_tool(
        &self,
        command: PublishToolCommand,
    ) -> Result<RegisteredToolView, ApplicationError>;
    async fn set_tool_enabled(
        &self,
        command: SetToolEnabledCommand,
    ) -> Result<RegisteredToolView, ApplicationError>;
    async fn get_registered_tool(
        &self,
        tool_id: &str,
        version: &str,
    ) -> Result<RegisteredToolView, ApplicationError>;
    async fn list_tool_descriptors(&self) -> Result<Vec<ToolDescriptorView>, ApplicationError>;
}
