use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    artifact::ArtifactRef,
    llm::ir::{LlmResponseIr, LlmStreamEventIr},
};

use super::super::{ContentGenerationKind, LlmOperationExecutionPin};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeAdapterKey {
    OpenAiResponsesV1,
    OpenAiChatCompletionsV1,
    ClaudeMessagesV1,
    GeminiGenerateContentV1,
}

impl ShapeAdapterKey {
    pub const fn generation_kind(self) -> ContentGenerationKind {
        match self {
            Self::OpenAiResponsesV1 => ContentGenerationKind::OpenAiResponses,
            Self::OpenAiChatCompletionsV1 => ContentGenerationKind::OpenAiChatCompletions,
            Self::ClaudeMessagesV1 => ContentGenerationKind::ClaudeMessages,
            Self::GeminiGenerateContentV1 => ContentGenerationKind::GeminiGenerateContent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterExecutionOptions {
    pub stream: bool,
    pub max_output_tokens: u64,
}

pub struct ResolvedArtifactMaterial {
    pub artifact_ref: ArtifactRef,
    pub bytes: Vec<u8>,
}

#[derive(Default)]
pub struct AdapterResources {
    pub materials: BTreeMap<String, ResolvedArtifactMaterial>,
    pub opaque_entries: BTreeMap<String, Vec<u8>>,
}

pub struct WireGenerationRequest {
    pub adapter_key: ShapeAdapterKey,
    pub operation: LlmOperationExecutionPin,
    pub method: gproxy_protocol::HttpMethod,
    pub relative_path: String,
    pub query: Option<String>,
    pub content_type: &'static str,
    body: Vec<u8>,
}

impl WireGenerationRequest {
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn from_parts(
        adapter_key: ShapeAdapterKey,
        operation: LlmOperationExecutionPin,
        method: gproxy_protocol::HttpMethod,
        relative_path: String,
        query: Option<String>,
        body: Vec<u8>,
    ) -> Self {
        Self {
            adapter_key,
            operation,
            method,
            relative_path,
            query,
            content_type: "application/json",
            body,
        }
    }
}

pub struct SensitiveEntryDraft {
    pub entry_key: String,
    pub adapter_key: ShapeAdapterKey,
    pub semantic_slot: String,
    pub opaque_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpaqueAttachmentTarget {
    ResponseContinuation,
    Item { item_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueAttachmentDraft {
    pub entry_key: String,
    pub target: OpaqueAttachmentTarget,
}

pub struct DecodedTerminalDraft {
    pub response: LlmResponseIr,
    pub sensitive_entries: Vec<SensitiveEntryDraft>,
    pub opaque_attachments: Vec<OpaqueAttachmentDraft>,
}

#[derive(Default)]
pub struct DecodedStreamBatch {
    pub events: Vec<LlmStreamEventIr>,
    pub sensitive_entries: Vec<SensitiveEntryDraft>,
    pub opaque_attachments: Vec<OpaqueAttachmentDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct ShapeAdapterError {
    pub code: &'static str,
    pub message: String,
}

impl ShapeAdapterError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
