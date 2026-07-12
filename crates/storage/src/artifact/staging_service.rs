use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        artifact::{
            ArtifactDownload, ArtifactListView, ArtifactStagingService,
            CommitArtifactStagingCommand, CompleteArtifactStagingCommand,
            CreateArtifactStagingCommand,
        },
    },
    artifact::{ArtifactStagingView, ArtifactView},
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

    async fn commit_artifact_staging(
        &self,
        command: CommitArtifactStagingCommand,
    ) -> Result<ArtifactView, ApplicationError> {
        self.commit_artifact_staging_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn get_artifact(&self, artifact_id: &str) -> Result<ArtifactView, ApplicationError> {
        self.get_artifact_view(artifact_id)
            .await
            .map_err(Into::into)
    }

    async fn list_artifacts(&self, limit: u32) -> Result<ArtifactListView, ApplicationError> {
        self.list_artifact_views(limit).await.map_err(Into::into)
    }

    async fn download_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<ArtifactDownload, ApplicationError> {
        self.download_artifact_value(artifact_id)
            .await
            .map_err(Into::into)
    }
}
