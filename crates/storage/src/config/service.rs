use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        channel::{
            ChannelService, ChannelView, CreateChannelCommand, PublishChannelRevisionCommand,
        },
        preset::{
            ContextPresetService, ContextPresetView, CreateContextPresetCommand,
            PublishContextPresetVersionCommand,
        },
        tool::{
            PublishToolCommand, RegisteredToolView, SetToolEnabledCommand, ToolDescriptorView,
            ToolRegistryService,
        },
    },
    llm::{LlmChannelRevision, context::ContextPresetVersion},
};

use crate::SqliteStore;

#[async_trait]
impl ChannelService for SqliteStore {
    async fn create_channel(
        &self,
        command: CreateChannelCommand,
    ) -> Result<ChannelView, ApplicationError> {
        SqliteStore::create_channel(self, command)
            .await
            .map_err(Into::into)
    }
    async fn list_channels(&self) -> Result<Vec<ChannelView>, ApplicationError> {
        SqliteStore::list_channels(self).await.map_err(Into::into)
    }
    async fn get_channel(&self, channel_id: &str) -> Result<ChannelView, ApplicationError> {
        SqliteStore::get_channel(self, channel_id)
            .await
            .map_err(Into::into)
    }
    async fn publish_channel_revision(
        &self,
        command: PublishChannelRevisionCommand,
    ) -> Result<LlmChannelRevision, ApplicationError> {
        SqliteStore::publish_channel_revision(self, command)
            .await
            .map_err(Into::into)
    }
    async fn get_channel_revision(
        &self,
        revision_id: &str,
    ) -> Result<LlmChannelRevision, ApplicationError> {
        SqliteStore::get_channel_revision(self, revision_id)
            .await
            .map_err(Into::into)
    }
    async fn get_channel_head_revision(
        &self,
        channel_id: &str,
    ) -> Result<LlmChannelRevision, ApplicationError> {
        SqliteStore::get_channel_head_revision(self, channel_id)
            .await
            .map_err(Into::into)
    }
}

#[async_trait]
impl ToolRegistryService for SqliteStore {
    async fn publish_tool(
        &self,
        command: PublishToolCommand,
    ) -> Result<RegisteredToolView, ApplicationError> {
        SqliteStore::publish_tool(self, command)
            .await
            .map_err(Into::into)
    }

    async fn set_tool_enabled(
        &self,
        command: SetToolEnabledCommand,
    ) -> Result<RegisteredToolView, ApplicationError> {
        SqliteStore::set_tool_enabled(self, command)
            .await
            .map_err(Into::into)
    }

    async fn get_registered_tool(
        &self,
        tool_id: &str,
        version: &str,
    ) -> Result<RegisteredToolView, ApplicationError> {
        SqliteStore::get_registered_tool(self, tool_id, version)
            .await
            .map_err(Into::into)
    }

    async fn list_tool_descriptors(&self) -> Result<Vec<ToolDescriptorView>, ApplicationError> {
        SqliteStore::list_tool_descriptors(self)
            .await
            .map_err(Into::into)
    }
}

#[async_trait]
impl ContextPresetService for SqliteStore {
    async fn create_context_preset(
        &self,
        command: CreateContextPresetCommand,
    ) -> Result<ContextPresetView, ApplicationError> {
        SqliteStore::create_context_preset(self, command)
            .await
            .map_err(Into::into)
    }
    async fn list_context_presets(&self) -> Result<Vec<ContextPresetView>, ApplicationError> {
        SqliteStore::list_context_presets(self)
            .await
            .map_err(Into::into)
    }
    async fn get_context_preset(
        &self,
        preset_id: &str,
    ) -> Result<ContextPresetView, ApplicationError> {
        SqliteStore::get_context_preset(self, preset_id)
            .await
            .map_err(Into::into)
    }
    async fn publish_context_preset_version(
        &self,
        command: PublishContextPresetVersionCommand,
    ) -> Result<ContextPresetVersion, ApplicationError> {
        SqliteStore::publish_context_preset_version(self, command)
            .await
            .map_err(Into::into)
    }
    async fn get_context_preset_version(
        &self,
        version_id: &str,
    ) -> Result<ContextPresetVersion, ApplicationError> {
        SqliteStore::get_context_preset_version(self, version_id)
            .await
            .map_err(Into::into)
    }
    async fn get_context_preset_head(
        &self,
        preset_id: &str,
    ) -> Result<ContextPresetVersion, ApplicationError> {
        SqliteStore::get_context_preset_head(self, preset_id)
            .await
            .map_err(Into::into)
    }
}
