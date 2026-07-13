use tauri::State;
use zhuangsheng_core::runtime::RunOutputsView;
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn get_run_outputs(
    state: State<'_, TauriAdapter>,
    run_id: String,
) -> CommandResult<RunOutputsView> {
    state.get_run_outputs(&run_id).await
}

#[tauri::command]
pub async fn load_json_value_bytes(
    state: State<'_, TauriAdapter>,
    value_ref: String,
) -> CommandResult<Vec<u8>> {
    state.load_json_value_bytes(&value_ref).await
}
