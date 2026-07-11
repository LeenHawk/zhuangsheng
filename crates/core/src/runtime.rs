use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::ApplicationError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RunContextCommand {
    Temporary,
    Existing {
        context_id: String,
        branch_id: String,
        expected_head_commit_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunCommand {
    pub graph_revision_id: String,
    pub input: Value,
    pub context: RunContextCommand,
    pub deadline_at: Option<i64>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunControlCommand {
    pub run_id: String,
    pub expected_epoch: u64,
    pub idempotency_key: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Running,
    Waiting,
    Interrupting,
    Interrupted,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub graph_revision_id: String,
    pub status: RunStatus,
    pub control_epoch: u64,
    pub context_id: String,
    pub branch_id: String,
    pub input_commit_id: String,
    pub input_ref: String,
    pub output_commit_id: Option<String>,
    pub last_durable_seq: u64,
    pub deadline_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBranchView {
    pub context_id: String,
    pub branch_id: String,
    pub head_commit_id: String,
    pub fork_commit_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RunOutputValueView {
    InlineJson {
        value_ref: String,
        content_hash: String,
        size_bytes: u64,
        value: Value,
    },
    JsonValueRef {
        value_ref: String,
        content_hash: String,
        size_bytes: u64,
        download_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutputEntryView {
    pub collection: String,
    pub values: Vec<RunOutputValueView>,
}

pub type RunOutputsView = BTreeMap<String, RunOutputEntryView>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableRunEventView {
    pub id: String,
    pub run_id: String,
    pub durable_seq: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub schema_version: u32,
    pub timestamp: i64,
    pub node_instance_id: Option<String>,
    pub attempt_id: Option<String>,
    pub importance: String,
    pub payload: Value,
}

#[async_trait]
pub trait RuntimeService: Send + Sync {
    async fn start_run(&self, command: StartRunCommand) -> Result<RunView, ApplicationError>;
    async fn get_run(&self, run_id: &str) -> Result<RunView, ApplicationError>;
    async fn get_run_outputs(&self, run_id: &str) -> Result<RunOutputsView, ApplicationError>;
    async fn list_run_events(
        &self,
        run_id: &str,
        after_durable_seq: u64,
        limit: u32,
    ) -> Result<Vec<DurableRunEventView>, ApplicationError>;
    async fn load_json_value_bytes(&self, value_ref: &str) -> Result<Vec<u8>, ApplicationError>;
    async fn request_interrupt(
        &self,
        command: RunControlCommand,
    ) -> Result<RunView, ApplicationError>;
    async fn resume_interrupted(
        &self,
        command: RunControlCommand,
    ) -> Result<RunView, ApplicationError>;
    async fn request_cancel(&self, command: RunControlCommand)
    -> Result<RunView, ApplicationError>;
}
