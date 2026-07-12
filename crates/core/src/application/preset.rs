use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::context::{
    ContextAssemblySnapshot, ContextAssemblySpec, ContextBudgetAction, ContextBudgetInput,
    ContextBudgetReport, ContextCountSource, ContextPresetVersion, ContextRole, ContextSource,
    PreviewContent, ResolvedContextBinding,
};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPresetView {
    pub id: String,
    pub name: String,
    pub head_version_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateContextPresetCommand {
    pub name: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishContextPresetVersionCommand {
    pub preset_id: String,
    pub expected_head_version_id: Option<String>,
    pub spec: ContextAssemblySpec,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewContextPresetCommand {
    pub preset_id: String,
    pub version_id: Option<String>,
    pub node_input: Value,
    #[serde(default)]
    pub sample_bindings: BTreeMap<String, ResolvedContextBinding>,
    pub budget: ContextBudgetInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreviewItemView {
    pub item_id: String,
    pub name: Option<String>,
    pub source_type: String,
    pub requested_role: ContextRole,
    pub enabled: bool,
    pub included: bool,
    pub token_count: u64,
    pub action: ContextBudgetAction,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPresetPreviewView {
    pub preset_id: String,
    pub version_id: String,
    pub content_mode: PreviewContent,
    pub count_source: ContextCountSource,
    pub items: Vec<ContextPreviewItemView>,
    pub budget_report: ContextBudgetReport,
    pub snapshot: ContextAssemblySnapshot,
}

#[async_trait]
pub trait ContextPresetService: Send + Sync {
    async fn create_context_preset(
        &self,
        command: CreateContextPresetCommand,
    ) -> Result<ContextPresetView, ApplicationError>;
    async fn list_context_presets(&self) -> Result<Vec<ContextPresetView>, ApplicationError>;
    async fn get_context_preset(
        &self,
        preset_id: &str,
    ) -> Result<ContextPresetView, ApplicationError>;
    async fn publish_context_preset_version(
        &self,
        command: PublishContextPresetVersionCommand,
    ) -> Result<ContextPresetVersion, ApplicationError>;
    async fn get_context_preset_version(
        &self,
        version_id: &str,
    ) -> Result<ContextPresetVersion, ApplicationError>;
    async fn get_context_preset_head(
        &self,
        preset_id: &str,
    ) -> Result<ContextPresetVersion, ApplicationError>;
    async fn preview_context_preset(
        &self,
        command: PreviewContextPresetCommand,
    ) -> Result<ContextPresetPreviewView, ApplicationError>;
}

pub fn preview_items(
    spec: &ContextAssemblySpec,
    report: &ContextBudgetReport,
) -> Vec<ContextPreviewItemView> {
    spec.items
        .iter()
        .map(|item| {
            let result = report.items.iter().find(|value| value.item_id == item.id);
            ContextPreviewItemView {
                item_id: item.id.clone(),
                name: item.name.clone(),
                source_type: source_type(&item.source).into(),
                requested_role: item.requested_role,
                enabled: item.enabled,
                included: result.is_some_and(|value| value.included),
                token_count: result.map_or(0, |value| value.token_count),
                action: result.map_or(ContextBudgetAction::Dropped, |value| value.action),
                reason: result.and_then(|value| value.reason.clone()).or_else(|| {
                    result
                        .is_none()
                        .then(|| "source produced no preview candidates".into())
                }),
            }
        })
        .collect()
}

fn source_type(source: &ContextSource) -> &'static str {
    match source {
        ContextSource::Literal { .. } => "literal",
        ContextSource::Template { .. } => "template",
        ContextSource::Input { .. } => "input",
        ContextSource::Memory { .. } => "memory",
        ContextSource::WorkingMemory { .. } => "working_memory",
        ContextSource::State { .. } => "state",
        ContextSource::History { .. } => "history",
        ContextSource::WorldInfo { .. } => "world_info",
        ContextSource::Summary { .. } => "summary",
        ContextSource::ToolTrace { .. } => "tool_trace",
        ContextSource::EventTrace { .. } => "event_trace",
        ContextSource::Artifact { .. } => "artifact",
        ContextSource::BranchContext { .. } => "branch_context",
    }
}
