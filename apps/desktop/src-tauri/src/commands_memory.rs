use tauri::State;
use zhuangsheng_core::{
    application::memory::{
        ApplyMemoryProposalCommand, ListMemoryProposalsCommand, MemoryProposalListView,
        MemorySearchCommand, MemorySearchView,
    },
    memory::{LongTermMemoryRecordView, MemoryChangeProposalView},
};
use zhuangsheng_tauri_adapter::{
    CommandResult, DecideMemoryProposalInput, ProposeMemoryChangeInput, TauriAdapter,
};

#[tauri::command]
pub async fn list_memory_proposals(
    state: State<'_, TauriAdapter>,
    command: ListMemoryProposalsCommand,
) -> CommandResult<MemoryProposalListView> {
    state.list_memory_proposals(command).await
}

#[tauri::command]
pub async fn propose_memory_change(
    state: State<'_, TauriAdapter>,
    input: ProposeMemoryChangeInput,
) -> CommandResult<MemoryChangeProposalView> {
    state.propose_memory_change(input).await
}

#[tauri::command]
pub async fn decide_memory_proposal(
    state: State<'_, TauriAdapter>,
    input: DecideMemoryProposalInput,
) -> CommandResult<MemoryChangeProposalView> {
    state.decide_memory_proposal(input).await
}

#[tauri::command]
pub async fn apply_memory_proposal(
    state: State<'_, TauriAdapter>,
    command: ApplyMemoryProposalCommand,
) -> CommandResult<MemoryChangeProposalView> {
    state.apply_memory_proposal(command).await
}

#[tauri::command]
pub async fn get_memory_record(
    state: State<'_, TauriAdapter>,
    memory_id: String,
) -> CommandResult<LongTermMemoryRecordView> {
    state.get_memory_record(&memory_id).await
}

#[tauri::command]
pub async fn search_memory(
    state: State<'_, TauriAdapter>,
    command: MemorySearchCommand,
) -> CommandResult<MemorySearchView> {
    state.search_memory(command).await
}
