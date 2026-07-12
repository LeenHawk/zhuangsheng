use serde::{Deserialize, Serialize};

use crate::{
    application::memory::MemorySearchCommand, graph::MemoryToolGrant,
    memory::MemoryProposalChangeInput,
};

use super::LlmLoopCheckpoint;

pub const MEMORY_SEARCH_TOOL_ID: &str = "builtin.search_memory";
pub const MEMORY_SEARCH_TOOL_VERSION: &str = "1";
pub const MEMORY_SEARCH_BINDING_ID: &str = "memory.search_memory";
pub const MEMORY_SEARCH_TOOL_NAME: &str = "search_memory";
pub const MEMORY_PROPOSAL_TOOL_ID: &str = "builtin.propose_memory_change";
pub const MEMORY_PROPOSAL_TOOL_VERSION: &str = "1";
pub const MEMORY_PROPOSAL_BINDING_ID: &str = "memory.propose_memory_change";
pub const MEMORY_PROPOSAL_TOOL_NAME: &str = "propose_memory_change";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryProposalToolInput {
    pub scope_id: String,
    pub memory_id: Option<String>,
    pub expected_head_commit_id: Option<String>,
    pub change: MemoryProposalChangeInput,
    pub reason: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProposalToolCallDigestMaterial {
    pub input: MemoryProposalToolInput,
    pub grant: MemoryToolGrant,
    pub policy_version: u32,
}

impl MemoryProposalToolCallDigestMaterial {
    pub fn digest(&self) -> crate::DomainResult<String> {
        crate::canonical::hash(self)
    }
}

pub struct PrepareMemoryProposalToolBatchCommand {
    pub wait_id: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub model_call_id: String,
    pub calls: Vec<MemoryProposalToolCallCommand>,
    pub checkpoint: LlmLoopCheckpoint,
}

pub struct MemoryProposalToolCallCommand {
    pub tool_call_id: String,
    pub provider_call_id: Option<String>,
    pub call_index: u64,
    pub call_digest: String,
    pub input: MemoryProposalToolInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedMemoryProposalToolBatch {
    pub wait_id: String,
    pub proposal_ids: Vec<String>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchToolCallDigestMaterial {
    pub query: MemorySearchCommand,
    pub grant: MemoryToolGrant,
    pub policy_version: u32,
}

impl MemorySearchToolCallDigestMaterial {
    pub fn digest(&self) -> crate::DomainResult<String> {
        crate::canonical::hash(self)
    }
}
