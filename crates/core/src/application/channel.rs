use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmChannelRevision, LlmChannelRevisionSpec, OperationKey};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub id: String,
    pub name: String,
    pub head_revision_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChannelCommand {
    pub name: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishChannelRevisionCommand {
    pub channel_id: String,
    pub expected_head_revision_id: Option<String>,
    pub spec: LlmChannelRevisionSpec,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverChannelModelsCommand {
    pub channel_id: String,
    pub revision_id: Option<String>,
    pub operation_key: Option<OperationKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredChannelModel {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelModelDiscoveryView {
    pub channel_id: String,
    pub channel_revision_id: String,
    pub operation_key: OperationKey,
    pub models: Vec<DiscoveredChannelModel>,
}

#[async_trait]
pub trait ChannelService: Send + Sync {
    async fn create_channel(
        &self,
        command: CreateChannelCommand,
    ) -> Result<ChannelView, ApplicationError>;
    async fn list_channels(&self) -> Result<Vec<ChannelView>, ApplicationError>;
    async fn get_channel(&self, channel_id: &str) -> Result<ChannelView, ApplicationError>;
    async fn publish_channel_revision(
        &self,
        command: PublishChannelRevisionCommand,
    ) -> Result<LlmChannelRevision, ApplicationError>;
    async fn get_channel_revision(
        &self,
        revision_id: &str,
    ) -> Result<LlmChannelRevision, ApplicationError>;
    async fn get_channel_head_revision(
        &self,
        channel_id: &str,
    ) -> Result<LlmChannelRevision, ApplicationError>;
}

#[async_trait]
pub trait ChannelModelDiscoveryService: Send + Sync {
    async fn discover_models(
        &self,
        command: DiscoverChannelModelsCommand,
    ) -> Result<ChannelModelDiscoveryView, ApplicationError>;
}
