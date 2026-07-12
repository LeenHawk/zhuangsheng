use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::llm::ir::{
    ContextProvenanceIr, ContextSensitivity, ContextTrust, InstructionIr, LlmContentPartIr,
    MessageRole,
};

use super::{ContextConfigSnapshot, ContextRole};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssemblyInput {
    pub node_input: Value,
    pub config: ContextConfigSnapshot,
    pub bindings: BTreeMap<String, ResolvedContextBinding>,
    pub budget: ContextBudgetInput,
    pub read_set_ref: String,
    pub read_set_digest: String,
    #[serde(default)]
    pub allow_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedContextBinding {
    pub binding_id: String,
    pub scope: String,
    pub version: String,
    #[serde(default)]
    pub values: Vec<ResolvedContextValue>,
    pub template_value: Option<Value>,
    pub template_provenance: Option<ContextProvenance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ResolvedContextValue {
    Data {
        id: String,
        content_hash: String,
        content: Vec<LlmContentPartIr>,
        provenance: ContextProvenance,
        allowed_roles: Vec<ContextRole>,
        relevance_score_micros: Option<i64>,
        #[serde(default)]
        tags: Vec<String>,
    },
    HistoryMessage {
        message_id: String,
        turn_id: String,
        stable_order: u64,
        role: MessageRole,
        content_hash: String,
        content: Vec<LlmContentPartIr>,
        provenance: ContextProvenance,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextProvenance {
    pub source_type: String,
    pub source_id: String,
    pub trust: ContextTrust,
    pub sensitivity: ContextSensitivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCountSource {
    Provider,
    Local,
    Estimate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetInput {
    pub context_window_tokens: u64,
    pub reserved_output_tokens: u64,
    pub fixed_request_tokens: u64,
    pub safety_margin_tokens: u64,
    pub count_source: ContextCountSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssemblyOutput {
    pub instructions: Vec<InstructionIr>,
    pub messages: Vec<AssembledMessageIr>,
    pub provenance: Vec<ContextProvenanceIr>,
    pub budget_report: ContextBudgetReport,
    pub snapshot: ContextAssemblySnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssembledMessageIr {
    pub id: String,
    pub role: MessageRole,
    pub content: Vec<LlmContentPartIr>,
    pub provenance_id: String,
    #[serde(default)]
    pub placeholder: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetReport {
    pub available_input_tokens: u64,
    pub fixed_request_tokens: u64,
    pub assembled_tokens: u64,
    pub count_source: ContextCountSource,
    pub items: Vec<ContextBudgetItemReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetItemReport {
    pub item_id: String,
    pub included: bool,
    pub token_count: u64,
    pub action: ContextBudgetAction,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudgetAction {
    Kept,
    Dropped,
    Truncated,
    Deduped,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssemblySnapshot {
    pub config: ContextAssemblySnapshotConfig,
    pub read_set_ref: String,
    pub read_set_digest: String,
    pub resolved_bindings_digest: String,
    pub assembly_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContextAssemblySnapshotConfig {
    Preset {
        preset_id: String,
        version_id: String,
        version: u64,
        content_hash: String,
    },
    GraphInline {
        graph_revision_id: String,
        node_id: String,
        content_hash: String,
    },
}

pub trait ContextTokenCounter: Send + Sync {
    fn count(&self, role: ContextRole, content: &[LlmContentPartIr]) -> ContextAssemblyResult<u64>;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{code}: {message}")]
pub struct ContextAssemblyError {
    pub code: &'static str,
    pub message: String,
}

impl ContextAssemblyError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<crate::DomainError> for ContextAssemblyError {
    fn from(error: crate::DomainError) -> Self {
        Self::new("context_canonicalization_failed", error.to_string())
    }
}

pub type ContextAssemblyResult<T> = Result<T, ContextAssemblyError>;
