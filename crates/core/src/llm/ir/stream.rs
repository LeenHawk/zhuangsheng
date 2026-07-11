use serde::{Deserialize, Serialize};

use super::{LlmApiError, LlmResponseIr, LlmTurnItemIr, LlmUsageIr};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LlmStreamEventIr {
    Started {
        call_id: String,
        seq: u64,
    },
    TextDelta {
        call_id: String,
        seq: u64,
        item_id: String,
        text: String,
    },
    ReasoningDelta {
        call_id: String,
        seq: u64,
        item_id: String,
        text: String,
    },
    ToolCallDelta {
        call_id: String,
        seq: u64,
        item_id: String,
        tool_call_id: String,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    ToolCallCompleted {
        call_id: String,
        seq: u64,
        item: LlmTurnItemIr,
    },
    HostedToolEvent {
        call_id: String,
        seq: u64,
        item: LlmTurnItemIr,
    },
    Usage {
        call_id: String,
        seq: u64,
        usage: LlmUsageIr,
    },
    Completed {
        call_id: String,
        seq: u64,
        response: LlmResponseIr,
    },
    Failed {
        call_id: String,
        seq: u64,
        error: LlmApiError,
    },
}

impl LlmStreamEventIr {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Started { call_id, .. }
            | Self::TextDelta { call_id, .. }
            | Self::ReasoningDelta { call_id, .. }
            | Self::ToolCallDelta { call_id, .. }
            | Self::ToolCallCompleted { call_id, .. }
            | Self::HostedToolEvent { call_id, .. }
            | Self::Usage { call_id, .. }
            | Self::Completed { call_id, .. }
            | Self::Failed { call_id, .. } => call_id,
        }
    }

    pub fn seq(&self) -> u64 {
        match self {
            Self::Started { seq, .. }
            | Self::TextDelta { seq, .. }
            | Self::ReasoningDelta { seq, .. }
            | Self::ToolCallDelta { seq, .. }
            | Self::ToolCallCompleted { seq, .. }
            | Self::HostedToolEvent { seq, .. }
            | Self::Usage { seq, .. }
            | Self::Completed { seq, .. }
            | Self::Failed { seq, .. } => *seq,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. })
    }
}
