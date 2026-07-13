use serde::{Deserialize, Serialize};

use crate::llm::ir::LlmContentPartIr;

use super::{
    ConversationMessageRole, ConversationMessageSource, ConversationTurnView, ConversationView,
    TurnCandidateStatus,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListView {
    pub items: Vec<ConversationView>,
    pub attention: Vec<ConversationAttentionView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationAttentionKind {
    ToolApproval,
    HumanResponse,
    MemoryProposalReview,
    SecretStoreUnlocked,
    EffectResolution,
    ProjectionConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationAttentionView {
    pub conversation_id: String,
    pub run_id: String,
    pub wait_id: Option<String>,
    pub kind: ConversationAttentionKind,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessageView {
    pub id: String,
    pub turn_id: String,
    pub branch_id: String,
    pub commit_id: String,
    pub parent_message_id: Option<String>,
    pub role: ConversationMessageRole,
    pub source: ConversationMessageSource,
    pub content: Vec<LlmContentPartIr>,
    pub origin_run_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateProjectionErrorView {
    pub code: String,
    pub safe_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCandidateView {
    pub turn_id: String,
    pub run_id: String,
    pub branch_id: String,
    pub base_commit_id: String,
    pub reply_output_key: String,
    pub status: TurnCandidateStatus,
    pub assistant_message_id: Option<String>,
    pub candidate_commit_id: Option<String>,
    pub projection_error: Option<CandidateProjectionErrorView>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnDetailView {
    #[serde(flatten)]
    pub turn: ConversationTurnView,
    pub selected_run_id: Option<String>,
    pub candidates: Vec<ConversationCandidateView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelineView {
    pub conversation_id: String,
    pub active_branch_id: String,
    pub active_head_commit_id: String,
    pub messages: Vec<ConversationMessageView>,
    pub turns: Vec<ConversationTurnDetailView>,
}
