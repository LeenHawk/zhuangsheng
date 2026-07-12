use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        artifact::{
            ArtifactStagingService, CompleteArtifactStagingCommand, CreateArtifactStagingCommand,
        },
    },
    artifact::ArtifactStagingView,
};

use crate::{SqliteStore, graph::helpers::now_ms};

#[async_trait]
impl ArtifactStagingService for SqliteStore {
    async fn create_artifact_staging(
        &self,
        command: CreateArtifactStagingCommand,
    ) -> Result<ArtifactStagingView, ApplicationError> {
        self.create_artifact_staging_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn complete_artifact_staging(
        &self,
        command: CompleteArtifactStagingCommand,
    ) -> Result<ArtifactStagingView, ApplicationError> {
        self.complete_artifact_staging_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn get_artifact_staging(
        &self,
        staging_id: &str,
    ) -> Result<ArtifactStagingView, ApplicationError> {
        self.get_artifact_staging_view(staging_id)
            .await
            .map_err(Into::into)
    }
}
