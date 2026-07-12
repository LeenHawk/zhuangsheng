use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        conversation::{
            ConversationService, CreateConversationCommand, UpdateConversationRunProfileCommand,
        },
    },
    conversation::{ConversationRunProfile, ConversationView},
};

use crate::{SqliteStore, graph::helpers::now_ms};

use super::read::load_conversation;

#[async_trait]
impl ConversationService for SqliteStore {
    async fn create_conversation(
        &self,
        command: CreateConversationCommand,
    ) -> Result<ConversationView, ApplicationError> {
        self.create_conversation_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn get_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationView, ApplicationError> {
        load_conversation(&self.db, conversation_id)
            .await
            .map_err(Into::into)
    }

    async fn update_run_profile(
        &self,
        command: UpdateConversationRunProfileCommand,
    ) -> Result<ConversationRunProfile, ApplicationError> {
        self.update_conversation_run_profile_at(command, now_ms())
            .await
            .map_err(Into::into)
    }
}

impl SqliteStore {
    pub async fn get_conversation_view(
        &self,
        conversation_id: &str,
    ) -> crate::StorageResult<ConversationView> {
        load_conversation(&self.db, conversation_id).await
    }
}
