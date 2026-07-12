use zhuangsheng_core::{
    application::{
        channel::{ChannelView, CreateChannelCommand},
        conversation::{CreateConversationCommand, UpdateConversationRunProfileCommand},
        graph::{
            CreateGraphCommand, CreateGraphResult, CreateRolePlayTemplateCommand, GraphRevisionView,
        },
        preset::{ContextPresetView, CreateContextPresetCommand},
    },
    conversation::{
        ConversationRunProfile, ConversationView, RolePlayCompatibilityView,
        RolePlayGraphOptionView, RolePlaySettingsView,
    },
};

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn create_graph(
        &self,
        command: CreateGraphCommand,
    ) -> CommandResult<CreateGraphResult> {
        Ok(self.graph.create_graph(command).await?)
    }

    pub async fn create_roleplay_template(
        &self,
        command: CreateRolePlayTemplateCommand,
    ) -> CommandResult<GraphRevisionView> {
        Ok(self.graph.create_roleplay_template(command).await?)
    }

    pub async fn create_channel(
        &self,
        command: CreateChannelCommand,
    ) -> CommandResult<ChannelView> {
        Ok(self.channel.create_channel(command).await?)
    }

    pub async fn create_context_preset(
        &self,
        command: CreateContextPresetCommand,
    ) -> CommandResult<ContextPresetView> {
        Ok(self.preset.create_context_preset(command).await?)
    }

    pub async fn create_conversation(
        &self,
        command: CreateConversationCommand,
    ) -> CommandResult<ConversationView> {
        Ok(self.conversation.create_conversation(command).await?)
    }

    pub async fn get_roleplay_settings(
        &self,
        revision_id: &str,
    ) -> CommandResult<RolePlaySettingsView> {
        Ok(self.graph.get_roleplay_settings(revision_id).await?)
    }

    pub async fn get_graph_revision(&self, revision_id: &str) -> CommandResult<GraphRevisionView> {
        Ok(self.graph.get_graph_revision(revision_id).await?)
    }

    pub async fn list_roleplay_graph_options(&self) -> CommandResult<Vec<RolePlayGraphOptionView>> {
        Ok(self.graph.list_roleplay_graph_options().await?)
    }

    pub async fn get_roleplay_compatibility(
        &self,
        revision_id: &str,
    ) -> CommandResult<RolePlayCompatibilityView> {
        Ok(self.graph.get_roleplay_compatibility(revision_id).await?)
    }

    pub async fn update_conversation_run_profile(
        &self,
        command: UpdateConversationRunProfileCommand,
    ) -> CommandResult<ConversationRunProfile> {
        Ok(self.conversation.update_run_profile(command).await?)
    }
}
