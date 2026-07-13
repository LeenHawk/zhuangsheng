use zhuangsheng_core::application::context::{
    CommitContextPatchCommand, ContextCommitView, ContextDiffView, CreateVersionSnapshotCommand,
    VersionSnapshotView, WorkingContextView,
};
use zhuangsheng_core::state::{ActorKind, ActorRef};

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn commit_context_patch(
        &self,
        mut command: CommitContextPatchCommand,
    ) -> CommandResult<ContextCommitView> {
        command.patch.author = ActorRef {
            kind: ActorKind::User,
            id: Some("local-user".into()),
        };
        Ok(self.context.commit_context_patch(command).await?)
    }

    pub async fn get_working_context(
        &self,
        context_id: &str,
        branch_id: &str,
    ) -> CommandResult<WorkingContextView> {
        Ok(self
            .context
            .get_working_context(context_id, branch_id)
            .await?)
    }

    pub async fn get_context_at_commit(
        &self,
        commit_id: &str,
    ) -> CommandResult<WorkingContextView> {
        Ok(self.context.get_context_at_commit(commit_id).await?)
    }

    pub async fn list_context_commits(
        &self,
        context_id: &str,
    ) -> CommandResult<Vec<ContextCommitView>> {
        Ok(self.context.list_context_commits(context_id).await?)
    }

    pub async fn diff_context_commits(
        &self,
        context_id: &str,
        from_commit_id: &str,
        to_commit_id: &str,
    ) -> CommandResult<ContextDiffView> {
        Ok(self
            .context
            .diff_context_commits(context_id, from_commit_id, to_commit_id)
            .await?)
    }

    pub async fn create_version_snapshot(
        &self,
        command: CreateVersionSnapshotCommand,
    ) -> CommandResult<VersionSnapshotView> {
        Ok(self.context.create_version_snapshot(command).await?)
    }
}
