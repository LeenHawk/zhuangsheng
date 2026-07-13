use tauri::State;
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn invoke_exact_json(
    state: State<'_, TauriAdapter>,
    operation: String,
    payload_json: String,
) -> CommandResult<Vec<u8>> {
    Ok(state.invoke_exact_json(&operation, &payload_json).await)
}
