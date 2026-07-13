use tauri::State;
use zhuangsheng_core::application::tool::ToolDescriptorView;
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn list_tool_descriptors(
    state: State<'_, TauriAdapter>,
) -> CommandResult<Vec<ToolDescriptorView>> {
    state.list_tool_descriptors().await
}
