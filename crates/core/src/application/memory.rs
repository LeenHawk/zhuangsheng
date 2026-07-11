use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    memory::{
        LongTermMemoryRecordView, LongTermMemoryStatus, MemoryChangeProposalView,
        MemoryProposalChangeInput, MemoryProposalStatus,
    },
    state::ActorRef,
};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposeMemoryChangeCommand {
    pub scope_id: String,
    pub memory_id: Option<String>,
    pub expected_head_commit_id: Option<String>,
    pub change: MemoryProposalChangeInput,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub requested_by: ActorRef,
    pub idempotency_key: String,
    pub schema_version: u32,
    pub policy_version: u32,
    pub origin_run_id: Option<String>,
    pub origin_node_instance_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProposalDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideMemoryProposalCommand {
    pub proposal_id: String,
    pub expected_status: MemoryProposalStatus,
    pub decision: MemoryProposalDecision,
    pub actor: ActorRef,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyMemoryProposalCommand {
    pub proposal_id: String,
    pub expected_status: MemoryProposalStatus,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchCommand {
    pub scope_id: String,
    pub text: Option<String>,
    pub tags: Vec<String>,
    pub status: Option<LongTermMemoryStatus>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchView {
    pub records: Vec<LongTermMemoryRecordView>,
    pub truncated: bool,
    pub scope_snapshot_token: String,
}

#[async_trait]
pub trait MemoryService: Send + Sync {
    async fn propose_memory_change(
        &self,
        command: ProposeMemoryChangeCommand,
    ) -> Result<MemoryChangeProposalView, ApplicationError>;
    async fn decide_memory_proposal(
        &self,
        command: DecideMemoryProposalCommand,
    ) -> Result<MemoryChangeProposalView, ApplicationError>;
    async fn apply_memory_proposal(
        &self,
        command: ApplyMemoryProposalCommand,
    ) -> Result<MemoryChangeProposalView, ApplicationError>;
    async fn get_memory_record(
        &self,
        memory_id: &str,
    ) -> Result<LongTermMemoryRecordView, ApplicationError>;
    async fn search_memory(
        &self,
        command: MemorySearchCommand,
    ) -> Result<MemorySearchView, ApplicationError>;
}
