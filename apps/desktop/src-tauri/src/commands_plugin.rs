use tauri::State;
use zhuangsheng_core::application::plugin::*;
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn inspect_git_plugin_source(
    state: State<'_, TauriAdapter>,
    command: InspectGitPluginCommand,
) -> CommandResult<PluginCandidateView> {
    state.inspect_git_plugin_source(command).await
}

#[tauri::command]
pub async fn activate_plugin_candidate(
    state: State<'_, TauriAdapter>,
    command: ActivatePluginCandidateCommand,
) -> CommandResult<PluginInstallationView> {
    state.activate_plugin_candidate(command).await
}

#[tauri::command]
pub async fn list_plugins(
    state: State<'_, TauriAdapter>,
) -> CommandResult<Vec<PluginInstallationView>> {
    state.list_plugins().await
}

#[tauri::command]
pub async fn configure_plugin(
    state: State<'_, TauriAdapter>,
    command: ConfigurePluginCommand,
) -> CommandResult<PluginInstallationView> {
    state.configure_plugin(command).await
}

#[tauri::command]
pub async fn check_plugin_update(
    state: State<'_, TauriAdapter>,
    plugin_id: String,
) -> CommandResult<Option<PluginCandidateView>> {
    state.check_plugin_update(&plugin_id).await
}

#[tauri::command]
pub async fn rollback_plugin(
    state: State<'_, TauriAdapter>,
    command: RollbackPluginCommand,
) -> CommandResult<PluginInstallationView> {
    state.rollback_plugin(command).await
}

#[tauri::command]
pub async fn get_plugin_entrypoint(
    state: State<'_, TauriAdapter>,
    plugin_id: String,
) -> CommandResult<PluginEntrypointView> {
    state.get_plugin_entrypoint(&plugin_id).await
}
