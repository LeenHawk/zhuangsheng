use async_trait::async_trait;
use zhuangsheng_core::application::{
    ApplicationError,
    context::{
        CommitContextPatchCommand, ContextCommitView, ContextDiffView, ContextService,
        CreateVersionSnapshotCommand, VersionSnapshotView, WorkingContextView,
    },
};

use crate::SqliteStore;

use super::{
    diff::diff_values,
    query::{list_branches, list_commits, load_commit, load_context},
};

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

    async fn list_context_branches(
        &self,
        context_id: &str,
    ) -> Result<Vec<zhuangsheng_core::runtime::ContextBranchView>, ApplicationError> {
        list_branches(&self.db, context_id)
            .await
            .map_err(Into::into)
    }

    async fn list_context_commits(
        &self,
        context_id: &str,
    ) -> Result<Vec<ContextCommitView>, ApplicationError> {
        list_commits(&self.db, context_id).await.map_err(Into::into)
    }

    async fn diff_context_commits(
        &self,
        context_id: &str,
        from_commit_id: &str,
        to_commit_id: &str,
    ) -> Result<ContextDiffView, ApplicationError> {
        let from = self.get_context_at_commit(from_commit_id).await?;
        let to = self.get_context_at_commit(to_commit_id).await?;
        if from.context_id != context_id || to.context_id != context_id {
            return Err(ApplicationError::InvalidArgument {
                code: "context_path_mismatch",
                message: "diff commits do not belong to the requested context".into(),
            });
        }
        Ok(ContextDiffView {
            context_id: context_id.into(),
            from_commit_id: from_commit_id.into(),
            to_commit_id: to_commit_id.into(),
            changes: diff_values(&from.value, &to.value),
        })
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
