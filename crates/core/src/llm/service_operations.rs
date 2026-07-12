use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::artifact::ArtifactRef;

use super::{LlmNodeModelRef, LlmOperationExecutionPin, ir::LlmUsageIr, ir::MetadataValue};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageOperationPlan {
    pub model: LlmNodeModelRef,
    pub prompt_ref: String,
    #[serde(default)]
    pub options: BTreeMap<String, MetadataValue>,
    pub max_images: u32,
    pub max_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageOperationResult {
    pub operation: LlmOperationExecutionPin,
    pub artifacts: Vec<ArtifactRef>,
    pub usage: Option<LlmUsageIr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedImageOperation {
    pub operation: LlmOperationExecutionPin,
    pub plan: ImageOperationPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingInputRef {
    pub source_ref: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingOperationPlan {
    pub model: LlmNodeModelRef,
    pub inputs: Vec<EmbeddingInputRef>,
    pub dimensions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingVectorRef {
    pub source_content_hash: String,
    pub dimensions: u32,
    pub vector_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingOperationResult {
    pub operation: LlmOperationExecutionPin,
    pub vectors: Vec<EmbeddingVectorRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedEmbeddingOperation {
    pub operation: LlmOperationExecutionPin,
    pub plan: EmbeddingOperationPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactPurpose {
    Context,
    History,
    Trace,
    MemoryProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactOperationPlan {
    pub model: LlmNodeModelRef,
    pub input_refs: Vec<String>,
    pub target_tokens: u64,
    pub purpose: CompactPurpose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactOperationResult {
    pub content_ref: String,
    pub source_refs: Vec<String>,
    pub operation: LlmOperationExecutionPin,
    pub token_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedCompactOperation {
    pub operation: LlmOperationExecutionPin,
    pub plan: CompactOperationPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct ServiceOperationError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub outcome_unknown: bool,
}

#[async_trait]
pub trait ImageOperationClient: Send + Sync {
    async fn create_image(
        &self,
        request: PreparedImageOperation,
    ) -> Result<ImageOperationResult, ServiceOperationError>;
}

#[async_trait]
pub trait EmbeddingOperationClient: Send + Sync {
    async fn create_embeddings(
        &self,
        request: PreparedEmbeddingOperation,
    ) -> Result<EmbeddingOperationResult, ServiceOperationError>;
}

#[async_trait]
pub trait CompactOperationClient: Send + Sync {
    async fn compact(
        &self,
        request: PreparedCompactOperation,
    ) -> Result<CompactOperationResult, ServiceOperationError>;
}
