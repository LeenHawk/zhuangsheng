use serde::{Deserialize, Serialize};
use zhuangsheng_core::{
    application::memory::{
        ApplyMemoryProposalCommand, DecideMemoryProposalCommand, ListMemoryProposalsCommand,
        MemoryProposalDecision, MemoryProposalListView, MemorySearchCommand, MemorySearchView,
        ProposeMemoryChangeCommand,
    },
    memory::{
        LongTermMemoryRecordView, MemoryChangeProposalView, MemoryProposalChangeInput,
        MemoryProposalStatus,
    },
    state::{ActorKind, ActorRef},
};

use crate::{CommandResult, TauriAdapter};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposeMemoryChangeInput {
    pub scope_id: String,
    pub memory_id: Option<String>,
    pub expected_head_commit_id: Option<String>,
    pub change: MemoryProposalChangeInput,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideMemoryProposalInput {
    pub proposal_id: String,
    pub expected_status: MemoryProposalStatus,
    pub decision: MemoryProposalDecision,
    pub idempotency_key: String,
}

impl TauriAdapter {
    pub async fn list_memory_proposals(
        &self,
        command: ListMemoryProposalsCommand,
    ) -> CommandResult<MemoryProposalListView> {
        Ok(self.memory.list_memory_proposals(command).await?)
    }

    pub async fn propose_memory_change(
        &self,
        input: ProposeMemoryChangeInput,
    ) -> CommandResult<MemoryChangeProposalView> {
        Ok(self
            .memory
            .propose_memory_change(ProposeMemoryChangeCommand {
                scope_id: input.scope_id,
                memory_id: input.memory_id,
                expected_head_commit_id: input.expected_head_commit_id,
                change: input.change,
                reason: input.reason,
                evidence_refs: input.evidence_refs,
                requested_by: local_actor(),
                idempotency_key: input.idempotency_key,
                schema_version: 1,
                policy_version: 1,
                origin_run_id: None,
                origin_node_instance_id: None,
            })
            .await?)
    }

    pub async fn decide_memory_proposal(
        &self,
        input: DecideMemoryProposalInput,
    ) -> CommandResult<MemoryChangeProposalView> {
        Ok(self
            .memory
            .decide_memory_proposal(DecideMemoryProposalCommand {
                proposal_id: input.proposal_id,
                expected_status: input.expected_status,
                decision: input.decision,
                actor: local_actor(),
                idempotency_key: input.idempotency_key,
            })
            .await?)
    }

    pub async fn apply_memory_proposal(
        &self,
        command: ApplyMemoryProposalCommand,
    ) -> CommandResult<MemoryChangeProposalView> {
        Ok(self.memory.apply_memory_proposal(command).await?)
    }

    pub async fn get_memory_record(
        &self,
        memory_id: &str,
    ) -> CommandResult<LongTermMemoryRecordView> {
        Ok(self.memory.get_memory_record(memory_id).await?)
    }

    pub async fn search_memory(
        &self,
        command: MemorySearchCommand,
    ) -> CommandResult<MemorySearchView> {
        Ok(self.memory.search_memory(command).await?)
    }
}

fn local_actor() -> ActorRef {
    ActorRef {
        kind: ActorKind::User,
        id: Some("local-user".into()),
    }
}
