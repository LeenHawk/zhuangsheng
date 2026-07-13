use tauri::State;
use zhuangsheng_core::{
    application::{
        channel::{
            ChannelModelDiscoveryView, ChannelView, DiscoverChannelModelsCommand,
            PublishChannelRevisionCommand,
        },
        graph::{CreateRolePlayTemplateCommand, GraphRevisionView},
        preset::{
            ContextPresetPreviewView, ContextPresetView, PreviewContextPresetCommand,
            PublishContextPresetVersionCommand,
        },
    },
    llm::{LlmChannelRevision, context::ContextPresetVersion},
};
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn list_channels(state: State<'_, TauriAdapter>) -> CommandResult<Vec<ChannelView>> {
    state.list_channels().await
}

#[tauri::command]
pub async fn get_channel(
    state: State<'_, TauriAdapter>,
    channel_id: String,
) -> CommandResult<ChannelView> {
    state.get_channel(&channel_id).await
}

#[tauri::command]
pub async fn publish_channel_revision(
    state: State<'_, TauriAdapter>,
    command: PublishChannelRevisionCommand,
) -> CommandResult<LlmChannelRevision> {
    state.publish_channel_revision(command).await
}

#[tauri::command]
pub async fn get_channel_revision(
    state: State<'_, TauriAdapter>,
    revision_id: String,
) -> CommandResult<LlmChannelRevision> {
    state.get_channel_revision(&revision_id).await
}

#[tauri::command]
pub async fn get_channel_head_revision(
    state: State<'_, TauriAdapter>,
    channel_id: String,
) -> CommandResult<LlmChannelRevision> {
    state.get_channel_head_revision(&channel_id).await
}

#[tauri::command]
pub async fn discover_channel_models(
    state: State<'_, TauriAdapter>,
    command: DiscoverChannelModelsCommand,
) -> CommandResult<ChannelModelDiscoveryView> {
    state.discover_channel_models(command).await
}

#[tauri::command]
pub async fn list_context_presets(
    state: State<'_, TauriAdapter>,
) -> CommandResult<Vec<ContextPresetView>> {
    state.list_context_presets().await
}

#[tauri::command]
pub async fn get_context_preset(
    state: State<'_, TauriAdapter>,
    preset_id: String,
) -> CommandResult<ContextPresetView> {
    state.get_context_preset(&preset_id).await
}

#[tauri::command]
pub async fn publish_context_preset_version(
    state: State<'_, TauriAdapter>,
    command: PublishContextPresetVersionCommand,
) -> CommandResult<ContextPresetVersion> {
    state.publish_context_preset_version(command).await
}

#[tauri::command]
pub async fn get_context_preset_version(
    state: State<'_, TauriAdapter>,
    version_id: String,
) -> CommandResult<ContextPresetVersion> {
    state.get_context_preset_version(&version_id).await
}

#[tauri::command]
pub async fn get_context_preset_head(
    state: State<'_, TauriAdapter>,
    preset_id: String,
) -> CommandResult<ContextPresetVersion> {
    state.get_context_preset_head(&preset_id).await
}

#[tauri::command]
pub async fn preview_context_preset(
    state: State<'_, TauriAdapter>,
    command: PreviewContextPresetCommand,
) -> CommandResult<ContextPresetPreviewView> {
    state.preview_context_preset(command).await
}

#[tauri::command]
pub async fn create_roleplay_template(
    state: State<'_, TauriAdapter>,
    command: CreateRolePlayTemplateCommand,
) -> CommandResult<GraphRevisionView> {
    state.create_roleplay_template(command).await
}

#[tauri::command]
pub async fn get_graph_revision(
    state: State<'_, TauriAdapter>,
    revision_id: String,
) -> CommandResult<GraphRevisionView> {
    state.get_graph_revision(&revision_id).await
}
