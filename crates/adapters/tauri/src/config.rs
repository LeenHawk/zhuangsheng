use zhuangsheng_core::{
    application::{
        ApplicationError,
        channel::{
            ChannelModelDiscoveryView, ChannelView, DiscoverChannelModelsCommand,
            PublishChannelRevisionCommand,
        },
        preset::{
            ContextPresetPreviewView, ContextPresetView, PreviewContextPresetCommand,
            PublishContextPresetVersionCommand,
        },
    },
    llm::{LlmChannelRevision, context::ContextPresetVersion},
};

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn list_channels(&self) -> CommandResult<Vec<ChannelView>> {
        Ok(self.channel.list_channels().await?)
    }

    pub async fn get_channel(&self, channel_id: &str) -> CommandResult<ChannelView> {
        Ok(self.channel.get_channel(channel_id).await?)
    }

    pub async fn publish_channel_revision(
        &self,
        command: PublishChannelRevisionCommand,
    ) -> CommandResult<LlmChannelRevision> {
        Ok(self.channel.publish_channel_revision(command).await?)
    }

    pub async fn get_channel_revision(
        &self,
        revision_id: &str,
    ) -> CommandResult<LlmChannelRevision> {
        Ok(self.channel.get_channel_revision(revision_id).await?)
    }

    pub async fn get_channel_head_revision(
        &self,
        channel_id: &str,
    ) -> CommandResult<LlmChannelRevision> {
        Ok(self.channel.get_channel_head_revision(channel_id).await?)
    }

    pub async fn discover_channel_models(
        &self,
        command: DiscoverChannelModelsCommand,
    ) -> CommandResult<ChannelModelDiscoveryView> {
        let service = self
            .model_discovery
            .as_ref()
            .ok_or(ApplicationError::Unavailable)?;
        Ok(service.discover_models(command).await?)
    }

    pub async fn list_context_presets(&self) -> CommandResult<Vec<ContextPresetView>> {
        Ok(self.preset.list_context_presets().await?)
    }

    pub async fn get_context_preset(&self, preset_id: &str) -> CommandResult<ContextPresetView> {
        Ok(self.preset.get_context_preset(preset_id).await?)
    }

    pub async fn publish_context_preset_version(
        &self,
        command: PublishContextPresetVersionCommand,
    ) -> CommandResult<ContextPresetVersion> {
        Ok(self.preset.publish_context_preset_version(command).await?)
    }

    pub async fn get_context_preset_version(
        &self,
        version_id: &str,
    ) -> CommandResult<ContextPresetVersion> {
        Ok(self.preset.get_context_preset_version(version_id).await?)
    }

    pub async fn get_context_preset_head(
        &self,
        preset_id: &str,
    ) -> CommandResult<ContextPresetVersion> {
        Ok(self.preset.get_context_preset_head(preset_id).await?)
    }

    pub async fn preview_context_preset(
        &self,
        command: PreviewContextPresetCommand,
    ) -> CommandResult<ContextPresetPreviewView> {
        Ok(self.preset.preview_context_preset(command).await?)
    }
}
