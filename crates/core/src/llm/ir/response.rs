use serde::{Deserialize, Serialize};

use crate::{artifact::ArtifactRef, llm::OperationKey};

use super::{LlmTurnItemIr, OpaqueContinuationRef};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponseIr {
    pub model_call_id: String,
    #[serde(default)]
    pub items: Vec<LlmTurnItemIr>,
    pub usage: Option<LlmUsageIr>,
    pub finish_reason: Option<LlmFinishReason>,
    pub continuation: Option<OpaqueContinuationRef>,
    pub raw_response_ref: Option<ArtifactRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmFinishReason {
    Completed,
    ToolCalls,
    Length,
    ContentFilter,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmUsageIr {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmApiError {
    pub operation_key: OperationKey,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub status_code: Option<u16>,
    pub code: Option<String>,
    pub message: String,
    pub retryable: bool,
}
