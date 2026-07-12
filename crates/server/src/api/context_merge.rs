use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::HeaderMap,
    routing::post,
};
use serde::Deserialize;
use zhuangsheng_core::context_merge::{
    ExplicitMergeSelection, MergeContextCommand, MergeContextView, MergeSourceDisposition,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeContextBody {
    source_branch_id: String,
    target_branch_id: String,
    expected_source_head: String,
    expected_target_head: String,
    source_disposition: MergeSourceDisposition,
    #[serde(default)]
    selections: Vec<ExplicitMergeSelection>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/contexts/{context_id}/merges", post(merge_context))
}

async fn merge_context(
    State(state): State<AppState>,
    Path(context_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<MergeContextBody>, JsonRejection>,
) -> ApiResult<Json<MergeContextView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .runtime_service
            .merge_context(MergeContextCommand {
                context_id,
                source_branch_id: body.source_branch_id,
                target_branch_id: body.target_branch_id,
                expected_source_head: body.expected_source_head,
                expected_target_head: body.expected_target_head,
                source_disposition: body.source_disposition,
                selections: body.selections,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}
