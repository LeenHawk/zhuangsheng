use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::{LlmChannelRevision, LlmChannelRevisionSpec};

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
