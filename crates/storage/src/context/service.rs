use async_trait::async_trait;
use zhuangsheng_core::application::{
    ApplicationError,
    context::{
        CommitContextPatchCommand, ContextCommitView, ContextService, CreateVersionSnapshotCommand,
        VersionSnapshotView, WorkingContextView,
    },
};

use crate::SqliteStore;

use super::query::{load_commit, load_context};

#[async_trait]
impl ContextService for SqliteStore {
    async fn commit_context_patch(
        &self,
        command: CommitContextPatchCommand,
    ) -> Result<ContextCommitView, ApplicationError> {
        SqliteStore::commit_context_patch(self, command)
            .await
            .map_err(Into::into)
    }

    async fn get_working_context(
        &self,
        context_id: &str,
        branch_id: &str,
    ) -> Result<WorkingContextView, ApplicationError> {
        load_context(&self.db, context_id, branch_id)
            .await
            .map_err(Into::into)
    }

    async fn get_context_at_commit(
        &self,
        commit_id: &str,
    ) -> Result<WorkingContextView, ApplicationError> {
        SqliteStore::get_context_at_commit(self, commit_id)
            .await
            .map_err(Into::into)
    }

    async fn create_version_snapshot(
        &self,
        command: CreateVersionSnapshotCommand,
    ) -> Result<VersionSnapshotView, ApplicationError> {
        SqliteStore::create_version_snapshot(self, command)
            .await
            .map_err(Into::into)
    }
}

impl SqliteStore {
    pub async fn get_working_context(
        &self,
        context_id: &str,
        branch_id: &str,
    ) -> crate::StorageResult<WorkingContextView> {
        load_context(&self.db, context_id, branch_id).await
    }

    pub async fn get_context_commit(
        &self,
        commit_id: &str,
    ) -> crate::StorageResult<ContextCommitView> {
        load_commit(&self.db, commit_id).await
    }
}
