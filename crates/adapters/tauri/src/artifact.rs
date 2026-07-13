use serde::{Deserialize, Serialize};
use zhuangsheng_core::{
    application::artifact::{
        ArtifactListView, CommitArtifactStagingCommand, CompleteArtifactStagingCommand,
        CreateArtifactStagingCommand,
    },
    artifact::{ArtifactStagingView, ArtifactView},
};

use crate::{CommandResult, TauriAdapter};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteArtifactStagingInput {
    pub staging_id: String,
    pub expected_lifecycle_generation: u64,
    pub bytes: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDownloadView {
    pub artifact: ArtifactView,
    pub bytes: Vec<u8>,
}

impl TauriAdapter {
    pub async fn create_artifact_staging(
        &self,
        command: CreateArtifactStagingCommand,
    ) -> CommandResult<ArtifactStagingView> {
        Ok(self.artifact.create_artifact_staging(command).await?)
    }

    pub async fn complete_artifact_staging(
        &self,
        input: CompleteArtifactStagingInput,
    ) -> CommandResult<ArtifactStagingView> {
        Ok(self
            .artifact
            .complete_artifact_staging(CompleteArtifactStagingCommand {
                staging_id: input.staging_id,
                expected_lifecycle_generation: input.expected_lifecycle_generation,
                bytes: input.bytes,
            })
            .await?)
    }

    pub async fn get_artifact_staging(
        &self,
        staging_id: &str,
    ) -> CommandResult<ArtifactStagingView> {
        Ok(self.artifact.get_artifact_staging(staging_id).await?)
    }

    pub async fn commit_artifact_staging(
        &self,
        command: CommitArtifactStagingCommand,
    ) -> CommandResult<ArtifactView> {
        Ok(self.artifact.commit_artifact_staging(command).await?)
    }

    pub async fn get_artifact(&self, artifact_id: &str) -> CommandResult<ArtifactView> {
        Ok(self.artifact.get_artifact(artifact_id).await?)
    }

    pub async fn list_artifacts(&self, limit: u32) -> CommandResult<ArtifactListView> {
        Ok(self.artifact.list_artifacts(limit.min(100)).await?)
    }

    pub async fn download_artifact(
        &self,
        artifact_id: &str,
    ) -> CommandResult<ArtifactDownloadView> {
        let value = self.artifact.download_artifact(artifact_id).await?;
        Ok(ArtifactDownloadView {
            artifact: value.artifact,
            bytes: value.bytes,
        })
    }
}
