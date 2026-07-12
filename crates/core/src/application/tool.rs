use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    llm::{ResolvedToolDescriptor, ToolDescriptor},
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
