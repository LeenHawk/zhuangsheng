use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    conversation::{
        ConversationRunProfile, ConversationRunSpec, ConversationSelectionView,
        ConversationTurnView, ConversationView, TurnCandidateView,
    },
    llm::ir::LlmContentPartIr,
    runtime::RunView,
};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConversationCommand {
    pub title: Option<String>,
    pub default_run: Option<ConversationRunSpec>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConversationRunProfileCommand {
    pub conversation_id: String,
    pub expected_revision_no: u64,
    pub run: ConversationRunSpec,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitConversationTurnCommand {
    pub conversation_id: String,
    pub expected_head_commit_id: String,
    pub user_content: Vec<LlmContentPartIr>,
    pub run: ConversationRunSpec,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitConversationTurnResult {
    pub turn: ConversationTurnView,
    pub candidate: TurnCandidateView,
    pub run: RunView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectConversationCandidateCommand {
    pub turn_id: String,
    pub selected_run_id: String,
    pub expected_conversation_head_commit_id: String,
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
    async fn update_run_profile(
        &self,
        command: UpdateConversationRunProfileCommand,
    ) -> Result<ConversationRunProfile, ApplicationError>;
    async fn submit_turn(
        &self,
        command: SubmitConversationTurnCommand,
    ) -> Result<SubmitConversationTurnResult, ApplicationError>;
    async fn select_candidate(
        &self,
        command: SelectConversationCandidateCommand,
    ) -> Result<ConversationSelectionView, ApplicationError>;
}
