use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::llm::context::{ContextAssemblySpec, ContextPresetVersion};

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
}
