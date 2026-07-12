use tauri::State;
use zhuangsheng_core::{
    application::conversation::{
        RegenerateConversationCandidateCommand, RegenerateConversationCandidateResult,
        ResolveCandidateProjectionCommand, ResolveCandidateProjectionResult,
        SelectConversationCandidateCommand, SubmitConversationTurnCommand,
        SubmitConversationTurnResult,
    },
    conversation::{
        ConversationListView, ConversationSelectionView, ConversationTimelineView,
        ConversationTurnDetailView, ConversationView, RolePlayCompatibilityView,
        RolePlayGraphOptionView,
    },
    runtime::ContextBranchView,
};
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn list_conversations(
    state: State<'_, TauriAdapter>,
) -> CommandResult<ConversationListView> {
    state.list_conversations().await
}

#[tauri::command]
pub async fn get_conversation(
    state: State<'_, TauriAdapter>,
    conversation_id: String,
) -> CommandResult<ConversationView> {
    state.get_conversation(&conversation_id).await
}

#[tauri::command]
pub async fn get_conversation_timeline(
    state: State<'_, TauriAdapter>,
    conversation_id: String,
) -> CommandResult<ConversationTimelineView> {
    state.get_conversation_timeline(&conversation_id).await
}

#[tauri::command]
pub async fn get_turn_candidates(
    state: State<'_, TauriAdapter>,
    turn_id: String,
) -> CommandResult<ConversationTurnDetailView> {
    state.get_turn_candidates(&turn_id).await
}

#[tauri::command]
pub async fn submit_conversation_turn(
    state: State<'_, TauriAdapter>,
    command: SubmitConversationTurnCommand,
) -> CommandResult<SubmitConversationTurnResult> {
    state.submit_conversation_turn(command).await
}

#[tauri::command]
pub async fn select_conversation_candidate(
    state: State<'_, TauriAdapter>,
    command: SelectConversationCandidateCommand,
) -> CommandResult<ConversationSelectionView> {
    state.select_conversation_candidate(command).await
}

#[tauri::command]
pub async fn regenerate_conversation_candidate(
    state: State<'_, TauriAdapter>,
    command: RegenerateConversationCandidateCommand,
) -> CommandResult<RegenerateConversationCandidateResult> {
    state.regenerate_conversation_candidate(command).await
}

#[tauri::command]
pub async fn resolve_candidate_projection(
    state: State<'_, TauriAdapter>,
    command: ResolveCandidateProjectionCommand,
) -> CommandResult<ResolveCandidateProjectionResult> {
    state.resolve_candidate_projection(command).await
}

#[tauri::command]
pub async fn list_roleplay_graph_options(
    state: State<'_, TauriAdapter>,
) -> CommandResult<Vec<RolePlayGraphOptionView>> {
    state.list_roleplay_graph_options().await
}

#[tauri::command]
pub async fn get_roleplay_compatibility(
    state: State<'_, TauriAdapter>,
    revision_id: String,
) -> CommandResult<RolePlayCompatibilityView> {
    state.get_roleplay_compatibility(&revision_id).await
}

#[tauri::command]
pub async fn list_context_branches(
    state: State<'_, TauriAdapter>,
    context_id: String,
) -> CommandResult<Vec<ContextBranchView>> {
    state.list_context_branches(&context_id).await
}
