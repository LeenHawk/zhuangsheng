use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::ActorRef;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongTermMemoryContentV1 {
    pub schema_version: u32,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemoryProposalChangeInput {
    Create { content: LongTermMemoryContentV1 },
    ReplaceContent { content: LongTermMemoryContentV1 },
    MarkObsolete,
    DeleteTombstone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProposalChangeType {
    Create,
    ReplaceContent,
    MarkObsolete,
    DeleteTombstone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProposalStatus {
    Proposed,
    AwaitingConfirmation,
    AwaitingReview,
    Approved,
    Rejected,
    Applied,
    Conflicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryStatus {
    Reserved,
    Active,
    Obsolete,
    Deleted,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryChangeProposalView {
    pub id: String,
    pub scope_id: String,
    pub memory_id: String,
    pub expected_head_commit_id: Option<String>,
    pub change_type: MemoryProposalChangeType,
    pub content_ref: Option<String>,
    pub proposed_content: Option<LongTermMemoryContentV1>,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub requested_by: ActorRef,
    pub schema_version: u32,
    pub policy_version: u32,
    pub origin_run_id: Option<String>,
    pub origin_node_instance_id: Option<String>,
    pub applied_commit_id: Option<String>,
    pub status: MemoryProposalStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LongTermMemoryRecordView {
    pub id: String,
    pub scope_id: String,
    pub status: LongTermMemoryStatus,
    pub head_commit_id: Option<String>,
    pub content_ref: Option<String>,
    pub content: Option<LongTermMemoryContentV1>,
    pub created_at: i64,
    pub updated_at: i64,
}
