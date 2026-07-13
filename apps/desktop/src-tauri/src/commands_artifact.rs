use tauri::State;
use zhuangsheng_core::{
    application::artifact::{
        ArtifactListView, CommitArtifactStagingCommand, CreateArtifactStagingCommand,
    },
    artifact::{ArtifactStagingView, ArtifactView},
};
use zhuangsheng_tauri_adapter::{
    ArtifactDownloadView, CommandResult, CompleteArtifactStagingInput, TauriAdapter,
};

#[tauri::command]
pub async fn create_artifact_staging(
    state: State<'_, TauriAdapter>,
    command: CreateArtifactStagingCommand,
) -> CommandResult<ArtifactStagingView> {
    state.create_artifact_staging(command).await
}

#[tauri::command]
pub async fn complete_artifact_staging(
    state: State<'_, TauriAdapter>,
    input: CompleteArtifactStagingInput,
) -> CommandResult<ArtifactStagingView> {
    state.complete_artifact_staging(input).await
}

#[tauri::command]
pub async fn get_artifact_staging(
    state: State<'_, TauriAdapter>,
    staging_id: String,
) -> CommandResult<ArtifactStagingView> {
    state.get_artifact_staging(&staging_id).await
}

#[tauri::command]
pub async fn commit_artifact_staging(
    state: State<'_, TauriAdapter>,
    command: CommitArtifactStagingCommand,
) -> CommandResult<ArtifactView> {
    state.commit_artifact_staging(command).await
}

#[tauri::command]
pub async fn get_artifact(
    state: State<'_, TauriAdapter>,
    artifact_id: String,
) -> CommandResult<ArtifactView> {
    state.get_artifact(&artifact_id).await
}

#[tauri::command]
pub async fn list_artifacts(
    state: State<'_, TauriAdapter>,
    limit: u32,
) -> CommandResult<ArtifactListView> {
    state.list_artifacts(limit).await
}

#[tauri::command]
pub async fn download_artifact(
    state: State<'_, TauriAdapter>,
    artifact_id: String,
) -> CommandResult<ArtifactDownloadView> {
    state.download_artifact(&artifact_id).await
}
