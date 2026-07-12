use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::artifact::{ArtifactMetadataDraft, ArtifactStagingView};

use super::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateArtifactStagingCommand {
    pub context_id: Option<String>,
    pub node_attempt_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub metadata_draft: ArtifactMetadataDraft,
    pub declared_media_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteArtifactStagingCommand {
    pub staging_id: String,
    pub expected_lifecycle_generation: u64,
    pub bytes: Vec<u8>,
}

#[async_trait]
pub trait ArtifactStagingService: Send + Sync {
    async fn create_artifact_staging(
        &self,
        command: CreateArtifactStagingCommand,
    ) -> Result<ArtifactStagingView, ApplicationError>;
    async fn complete_artifact_staging(
        &self,
        command: CompleteArtifactStagingCommand,
    ) -> Result<ArtifactStagingView, ApplicationError>;
    async fn get_artifact_staging(
        &self,
        staging_id: &str,
    ) -> Result<ArtifactStagingView, ApplicationError>;
}
