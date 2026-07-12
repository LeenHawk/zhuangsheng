use crate::llm::ir::{ContextProvenanceIr, LlmContentPartIr};

use super::{ContextPosition, ContextRole, OverflowPolicy};

pub(super) struct CandidateGroup {
    pub item_id: String,
    pub item_index: usize,
    pub position: ContextPosition,
    pub order: i64,
    pub priority: i64,
    pub insertion_depth: u32,
    pub required: bool,
    pub max_tokens: Option<u64>,
    pub overflow: Option<OverflowPolicy>,
    pub candidates: Vec<ContextCandidate>,
    pub pre_action: Option<&'static str>,
}

#[derive(Clone)]
pub(super) struct ContextCandidate {
    pub id: String,
    pub sub_index: usize,
    pub history_order: Option<u64>,
    pub role: ContextRole,
    pub content: Vec<LlmContentPartIr>,
    pub provenance: ContextProvenanceIr,
    pub content_hash: String,
    pub relevance_score_micros: Option<i64>,
    pub included: bool,
    pub token_count: u64,
}
