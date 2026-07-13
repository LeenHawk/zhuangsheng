use tauri::State;
use zhuangsheng_core::application::graph::{
    ApplyGraphCommand, GraphDraftView, GraphRevisionView, GraphView, UpdateGraphDraftCommand,
};
use zhuangsheng_tauri_adapter::{CommandResult, TauriAdapter};

#[tauri::command]
pub async fn list_graphs(state: State<'_, TauriAdapter>) -> CommandResult<Vec<GraphView>> {
    state.list_graphs().await
}

#[tauri::command]
pub async fn get_graph_draft(
    state: State<'_, TauriAdapter>,
    graph_id: String,
) -> CommandResult<GraphDraftView> {
    state.get_graph_draft(&graph_id).await
}

#[tauri::command]
pub async fn update_graph_draft(
    state: State<'_, TauriAdapter>,
    command: UpdateGraphDraftCommand,
) -> CommandResult<GraphDraftView> {
    state.update_graph_draft(command).await
}

#[tauri::command]
pub async fn apply_graph(
    state: State<'_, TauriAdapter>,
    command: ApplyGraphCommand,
) -> CommandResult<GraphRevisionView> {
    state.apply_graph(command).await
}

#[tauri::command]
pub async fn get_graph_revision_for_graph(
    state: State<'_, TauriAdapter>,
    graph_id: String,
    revision_id: String,
) -> CommandResult<GraphRevisionView> {
    state
        .get_graph_revision_for_graph(&graph_id, &revision_id)
        .await
}
