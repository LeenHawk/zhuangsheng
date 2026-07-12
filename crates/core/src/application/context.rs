use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::{ActorRef, StatePatch};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitContextPatchCommand {
    pub patch: StatePatch,
    pub origin_run_id: Option<String>,
    pub origin_node_instance_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCommitView {
    pub id: String,
    pub context_id: String,
    pub branch_id: String,
    pub sequence_no: u64,
    pub operation_id: String,
    pub parent_commit_ids: Vec<String>,
    pub patch_ref: Option<String>,
    pub schema_version: u32,
    pub policy_version: u32,
    pub author: ActorRef,
    pub origin_run_id: Option<String>,
    pub origin_node_instance_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingContextView {
    pub context_id: String,
    pub branch_id: String,
    pub head_commit_id: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDiffEntry {
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDiffView {
    pub context_id: String,
    pub from_commit_id: String,
    pub to_commit_id: String,
    pub changes: Vec<ContextDiffEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVersionSnapshotCommand {
    pub commit_id: String,
    pub retention_until: Option<i64>,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSnapshotView {
    pub commit_id: String,
    pub snapshot_ref: String,
    pub checksum: String,
    pub schema_version: u32,
    pub retention_until: Option<i64>,
    pub pinned: bool,
    pub created_at: i64,
}

#[async_trait]
pub trait ContextService: Send + Sync {
    async fn commit_context_patch(
        &self,
        command: CommitContextPatchCommand,
    ) -> Result<ContextCommitView, ApplicationError>;

    async fn get_working_context(
        &self,
        context_id: &str,
        branch_id: &str,
    ) -> Result<WorkingContextView, ApplicationError>;

    async fn get_context_at_commit(
        &self,
        commit_id: &str,
    ) -> Result<WorkingContextView, ApplicationError>;

    async fn list_context_branches(
        &self,
        context_id: &str,
    ) -> Result<Vec<crate::runtime::ContextBranchView>, ApplicationError>;

    async fn list_context_commits(
        &self,
        context_id: &str,
    ) -> Result<Vec<ContextCommitView>, ApplicationError>;

    async fn diff_context_commits(
        &self,
        context_id: &str,
        from_commit_id: &str,
        to_commit_id: &str,
    ) -> Result<ContextDiffView, ApplicationError>;

    async fn create_version_snapshot(
        &self,
        command: CreateVersionSnapshotCommand,
    ) -> Result<VersionSnapshotView, ApplicationError>;
}
