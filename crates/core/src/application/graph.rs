use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    ValidationIssue,
    conversation::{RolePlayCompatibilityView, RolePlayGraphOptionView, RolePlaySettingsView},
    graph::{AppliedGraphDefinition, GraphDraft},
};

use super::ApplicationError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGraphCommand {
    pub name: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGraphResult {
    pub graph: GraphView,
    pub draft_revision_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDraftView {
    pub graph_id: String,
    pub document: GraphDraft,
    pub revision_token: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGraphDraftCommand {
    pub graph_id: String,
    pub expected_revision_token: String,
    pub document: GraphDraft,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGraphCommand {
    pub graph_id: String,
    pub expected_revision_token: String,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRolePlayTemplateCommand {
    pub name: String,
    pub channel_id: String,
    pub preset_id: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRevisionView {
    pub id: String,
    pub graph_id: String,
    pub revision_no: u64,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub definition: AppliedGraphDefinition,
    pub content_hash: String,
    pub created_at: i64,
    pub warnings: Vec<ValidationIssue>,
}

#[async_trait]
pub trait GraphService: Send + Sync {
    async fn create_graph(
        &self,
        command: CreateGraphCommand,
    ) -> Result<CreateGraphResult, ApplicationError>;
    async fn list_graphs(&self) -> Result<Vec<GraphView>, ApplicationError>;
    async fn get_graph_draft(&self, graph_id: &str) -> Result<GraphDraftView, ApplicationError>;
    async fn update_graph_draft(
        &self,
        command: UpdateGraphDraftCommand,
    ) -> Result<GraphDraftView, ApplicationError>;
    async fn apply_graph(
        &self,
        command: ApplyGraphCommand,
    ) -> Result<GraphRevisionView, ApplicationError>;
    async fn create_roleplay_template(
        &self,
        command: CreateRolePlayTemplateCommand,
    ) -> Result<GraphRevisionView, ApplicationError>;
    async fn get_graph_revision(
        &self,
        revision_id: &str,
    ) -> Result<GraphRevisionView, ApplicationError>;
    async fn get_graph_revision_for_graph(
        &self,
        graph_id: &str,
        revision_id: &str,
    ) -> Result<GraphRevisionView, ApplicationError>;
    async fn list_roleplay_graph_options(
        &self,
    ) -> Result<Vec<RolePlayGraphOptionView>, ApplicationError>;
    async fn get_roleplay_compatibility(
        &self,
        revision_id: &str,
    ) -> Result<RolePlayCompatibilityView, ApplicationError>;
    async fn get_roleplay_settings(
        &self,
        revision_id: &str,
    ) -> Result<RolePlaySettingsView, ApplicationError>;
}
