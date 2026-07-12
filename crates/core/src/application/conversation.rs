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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateConversationCandidateCommand {
    pub turn_id: String,
    pub expected_user_commit_id: String,
    pub run: ConversationRunSpec,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegenerateConversationCandidateResult {
    pub candidate: TurnCandidateView,
    pub run: RunView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CandidateProjectionResolution {
    AppendAfterCurrent { reason: String },
    AbandonProjection { reason: String },
}

impl CandidateProjectionResolution {
    pub fn reason(&self) -> &str {
        match self {
            Self::AppendAfterCurrent { reason } | Self::AbandonProjection { reason } => reason,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCandidateProjectionCommand {
    pub turn_id: String,
    pub run_id: String,
    pub expected_current_branch_head: String,
    pub resolution: CandidateProjectionResolution,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCandidateProjectionResult {
    pub turn_id: String,
    pub run_id: String,
    pub branch_id: String,
    pub branch_head_commit_id: String,
    pub status: crate::conversation::TurnCandidateStatus,
    pub assistant_message_id: Option<String>,
    pub candidate_commit_id: Option<String>,
    pub resolved_at: i64,
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
    async fn regenerate_candidate(
        &self,
        command: RegenerateConversationCandidateCommand,
    ) -> Result<RegenerateConversationCandidateResult, ApplicationError>;
    async fn resolve_candidate_projection(
        &self,
        command: ResolveCandidateProjectionCommand,
    ) -> Result<ResolveCandidateProjectionResult, ApplicationError>;
}
