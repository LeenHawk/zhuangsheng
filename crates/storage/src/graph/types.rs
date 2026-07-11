use serde::{Deserialize, Serialize};
use zhuangsheng_core::{
    ValidationIssue,
    graph::{AppliedGraphDefinition, GraphDraft},
};

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
