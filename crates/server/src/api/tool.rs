use axum::{Json, Router, extract::State, routing::get};
use zhuangsheng_core::application::tool::ToolDescriptorView;

use super::{AppState, error::ApiResult};

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/tools/descriptors", get(list_descriptors))
}

async fn list_descriptors(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<ToolDescriptorView>>> {
    Ok(Json(
        state.tool_registry_service.list_tool_descriptors().await?,
    ))
}
