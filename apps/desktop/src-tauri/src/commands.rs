use tauri::State;
use zhuangsheng_core::{
    application::{
        channel::{ChannelView, CreateChannelCommand},
        conversation::{CreateConversationCommand, UpdateConversationRunProfileCommand},
        graph::{CreateGraphCommand, CreateGraphResult},
        preset::{ContextPresetView, CreateContextPresetCommand},
    },
    context_merge::{MergeContextCommand, MergeContextView},
    conversation::{ConversationRunProfile, ConversationView, RolePlaySettingsView},
    llm::EffectResolutionView,
    runtime::{
        ContextBranchView, DurableRunEventView, ForkContextCommand, RunControlCommand, RunListView,
        RunView, StartRunCommand, WaitDeliveryView, WaitView,
    },
};
use zhuangsheng_tauri_adapter::{
    CommandResult, ResolveEffectUnknownInput, SatisfyWaitInput, TauriAdapter,
};

#[path = "commands_artifact.rs"]
pub mod artifact;
#[path = "commands_config.rs"]
pub mod config;
#[path = "commands_context.rs"]
pub mod context;
#[path = "commands_conversation.rs"]
pub mod conversation;
#[path = "commands_graph.rs"]
pub mod graph;
#[path = "commands_memory.rs"]
pub mod memory;
#[path = "commands_runtime_extra.rs"]
pub mod runtime_extra;
#[path = "commands_secret.rs"]
pub mod secret;
#[path = "commands_tool.rs"]
pub mod tool;

#[tauri::command]
pub async fn start_run(
    state: State<'_, TauriAdapter>,
    command: StartRunCommand,
) -> CommandResult<RunView> {
    state.start_run(command).await
}
#[tauri::command]
pub async fn get_run(state: State<'_, TauriAdapter>, run_id: String) -> CommandResult<RunView> {
    state.get_run(&run_id).await
}
#[tauri::command]
pub async fn list_recent_runs(
    state: State<'_, TauriAdapter>,
    limit: u32,
) -> CommandResult<RunListView> {
    state.list_recent_runs(limit).await
}
#[tauri::command]
pub async fn list_open_waits(
    state: State<'_, TauriAdapter>,
    run_id: String,
) -> CommandResult<Vec<WaitView>> {
    state.list_open_waits(&run_id).await
}
#[tauri::command]
pub async fn list_run_events(
    state: State<'_, TauriAdapter>,
    run_id: String,
    after_durable_seq: u64,
    limit: u32,
) -> CommandResult<Vec<DurableRunEventView>> {
    state
        .list_run_events(&run_id, after_durable_seq, limit)
        .await
}
#[tauri::command]
pub async fn interrupt_run(
    state: State<'_, TauriAdapter>,
    command: RunControlCommand,
) -> CommandResult<RunView> {
    state.interrupt_run(command).await
}
#[tauri::command]
pub async fn resume_run(
    state: State<'_, TauriAdapter>,
    command: RunControlCommand,
) -> CommandResult<RunView> {
    state.resume_run(command).await
}
#[tauri::command]
pub async fn cancel_run(
    state: State<'_, TauriAdapter>,
    command: RunControlCommand,
) -> CommandResult<RunView> {
    state.cancel_run(command).await
}
#[tauri::command]
pub async fn satisfy_wait(
    state: State<'_, TauriAdapter>,
    input: SatisfyWaitInput,
) -> CommandResult<WaitDeliveryView> {
    state.satisfy_wait(input).await
}
#[tauri::command]
pub async fn resolve_effect_unknown(
    state: State<'_, TauriAdapter>,
    input: ResolveEffectUnknownInput,
) -> CommandResult<EffectResolutionView> {
    state.resolve_effect_unknown(input).await
}
#[tauri::command]
pub async fn fork_context(
    state: State<'_, TauriAdapter>,
    command: ForkContextCommand,
) -> CommandResult<ContextBranchView> {
    state.fork_context(command).await
}
#[tauri::command]
pub async fn merge_context(
    state: State<'_, TauriAdapter>,
    command: MergeContextCommand,
) -> CommandResult<MergeContextView> {
    state.merge_context(command).await
}
#[tauri::command]
pub async fn create_graph(
    state: State<'_, TauriAdapter>,
    command: CreateGraphCommand,
) -> CommandResult<CreateGraphResult> {
    state.create_graph(command).await
}
#[tauri::command]
pub async fn create_channel(
    state: State<'_, TauriAdapter>,
    command: CreateChannelCommand,
) -> CommandResult<ChannelView> {
    state.create_channel(command).await
}
#[tauri::command]
pub async fn create_context_preset(
    state: State<'_, TauriAdapter>,
    command: CreateContextPresetCommand,
) -> CommandResult<ContextPresetView> {
    state.create_context_preset(command).await
}
#[tauri::command]
pub async fn create_conversation(
    state: State<'_, TauriAdapter>,
    command: CreateConversationCommand,
) -> CommandResult<ConversationView> {
    state.create_conversation(command).await
}
#[tauri::command]
pub async fn get_roleplay_settings(
    state: State<'_, TauriAdapter>,
    revision_id: String,
) -> CommandResult<RolePlaySettingsView> {
    state.get_roleplay_settings(&revision_id).await
}
#[tauri::command]
pub async fn update_conversation_run_profile(
    state: State<'_, TauriAdapter>,
    command: UpdateConversationRunProfileCommand,
) -> CommandResult<ConversationRunProfile> {
    state.update_conversation_run_profile(command).await
}
