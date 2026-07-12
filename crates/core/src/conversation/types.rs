use crate::llm::ir::LlmContentPartIr;
use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationContextV1 {
    pub schema_version: u32,
    pub messages: Vec<ConversationContextMessageV1>,
}

impl ConversationContextV1 {
    pub fn empty() -> Self {
        Self {
            schema_version: 1,
            messages: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || self.messages.len() > 100_000 {
            return Err("conversation context is invalid");
        }
        let mut seen = HashSet::new();
        for message in &self.messages {
            if message.message_id.is_empty()
                || message.message_id.len() > 128
                || message.turn_id.is_empty()
                || message.turn_id.len() > 128
                || message.content_ref.is_empty()
                || message.content_ref.len() > 128
                || seen.contains(message.message_id.as_str())
                || message
                    .parent_message_id
                    .as_ref()
                    .is_some_and(|parent| !seen.contains(parent.as_str()))
                || !message.provenance_is_valid()
            {
                return Err("conversation message is invalid");
            }
            seen.insert(message.message_id.as_str());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationContextMessageV1 {
    pub message_id: String,
    pub turn_id: String,
    pub role: ConversationMessageRole,
    pub source: ConversationMessageSource,
    pub content_ref: String,
    pub parent_message_id: Option<String>,
    pub origin_run_id: Option<String>,
}

impl ConversationContextMessageV1 {
    fn provenance_is_valid(&self) -> bool {
        match (self.role, self.source) {
            (ConversationMessageRole::User, ConversationMessageSource::UserInput) => {
                self.origin_run_id.is_none()
            }
            (
                ConversationMessageRole::Assistant,
                ConversationMessageSource::RunOutput | ConversationMessageSource::SavedPartial,
            ) => self.origin_run_id.is_some() && self.parent_message_id.is_some(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageSource {
    UserInput,
    RunOutput,
    SavedPartial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    pub id: String,
    pub title: Option<String>,
    pub context_id: String,
    pub active_branch_id: String,
    pub active_head_commit_id: String,
    pub run_profile: Option<ConversationRunProfile>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationInputShape {
    ConversationMessageV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunSpec {
    pub graph_revision_id: String,
    pub reply_output_key: String,
    pub input_shape: ConversationInputShape,
}

impl ConversationRunSpec {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.graph_revision_id.is_empty()
            || self.graph_revision_id.len() > 128
            || self.reply_output_key.is_empty()
            || self.reply_output_key.len() > 128
            || self.input_shape != ConversationInputShape::ConversationMessageV1
        {
            return Err("conversation run spec is invalid");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunProfile {
    #[serde(flatten)]
    pub run: ConversationRunSpec,
    pub revision_no: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRunInputV1 {
    pub schema_version: u32,
    pub conversation_id: String,
    pub turn_id: String,
    pub user_message_id: String,
    pub user_commit_id: String,
    pub content: Vec<LlmContentPartIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantReplyPayloadV1 {
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub payload_type: String,
    pub content: Vec<LlmContentPartIr>,
}

impl AssistantReplyPayloadV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || self.payload_type != "assistant_reply" {
            return Err("assistant reply payload is invalid");
        }
        crate::llm::ir::validate_content_parts(&self.content, true)
            .map_err(|_| "assistant reply content is invalid")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnView {
    pub id: String,
    pub conversation_id: String,
    pub user_message_id: String,
    pub user_commit_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnCandidateStatus {
    Running,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCandidateView {
    pub turn_id: String,
    pub run_id: String,
    pub branch_id: String,
    pub base_commit_id: String,
    pub reply_output_key: String,
    pub status: TurnCandidateStatus,
    pub created_at: i64,
}
