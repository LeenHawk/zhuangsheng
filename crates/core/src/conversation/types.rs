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
