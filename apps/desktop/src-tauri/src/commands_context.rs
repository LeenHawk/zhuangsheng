use tauri::State;
use zhuangsheng_core::application::context::{
    CommitContextPatchCommand, ContextCommitView, ContextDiffView, CreateVersionSnapshotCommand,
    VersionSnapshotView, WorkingContextView,
};
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn commit_context_patch(
    state: State<'_, TauriAdapter>,
    command: CommitContextPatchCommand,
) -> CommandResult<ContextCommitView> {
    state.commit_context_patch(command).await
}

#[tauri::command]
pub async fn get_working_context(
    state: State<'_, TauriAdapter>,
    context_id: String,
    branch_id: String,
) -> CommandResult<WorkingContextView> {
    state.get_working_context(&context_id, &branch_id).await
}

#[tauri::command]
pub async fn get_context_at_commit(
    state: State<'_, TauriAdapter>,
    commit_id: String,
) -> CommandResult<WorkingContextView> {
    state.get_context_at_commit(&commit_id).await
}

#[tauri::command]
pub async fn list_context_commits(
    state: State<'_, TauriAdapter>,
    context_id: String,
) -> CommandResult<Vec<ContextCommitView>> {
    state.list_context_commits(&context_id).await
}

#[tauri::command]
pub async fn diff_context_commits(
    state: State<'_, TauriAdapter>,
    context_id: String,
    from_commit_id: String,
    to_commit_id: String,
) -> CommandResult<ContextDiffView> {
    state
        .diff_context_commits(&context_id, &from_commit_id, &to_commit_id)
        .await
}

#[tauri::command]
pub async fn create_version_snapshot(
    state: State<'_, TauriAdapter>,
    command: CreateVersionSnapshotCommand,
) -> CommandResult<VersionSnapshotView> {
    state.create_version_snapshot(command).await
}
