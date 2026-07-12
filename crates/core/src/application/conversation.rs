use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::conversation::ConversationView;

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConversationCommand {
    pub title: Option<String>,
    pub idempotency_key: String,
}

#[async_trait]
pub trait ConversationService: Send + Sync {
    async fn create_conversation(
        &self,
        command: CreateConversationCommand,
    ) -> Result<ConversationView, ApplicationError>;
    async fn get_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationView, ApplicationError>;
}
