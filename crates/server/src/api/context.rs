use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::application::context::{
    CommitContextPatchCommand, ContextCommitView, CreateVersionSnapshotCommand,
    VersionSnapshotView, WorkingContextView,
};

use super::{
    AppState,
    error::{ApiError, ApiResult},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotBody {
    retention_until: Option<i64>,
    #[serde(default)]
    pinned: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/contexts/{context_id}/branches/{branch_id}",
            get(get_context),
        )
        .route(
            "/v1/contexts/{context_id}/branches/{branch_id}/commits",
            post(commit_patch),
        )
        .route(
            "/v1/context-commits/{commit_id}",
            get(get_context_at_commit),
        )
        .route(
            "/v1/context-commits/{commit_id}/snapshot",
            post(create_snapshot),
        )
}

async fn get_context(
    State(state): State<AppState>,
    Path((context_id, branch_id)): Path<(String, String)>,
) -> ApiResult<Json<WorkingContextView>> {
    Ok(Json(
        state
            .context_service
            .get_working_context(&context_id, &branch_id)
            .await?,
    ))
}

async fn commit_patch(
    State(state): State<AppState>,
    Path((context_id, branch_id)): Path<(String, String)>,
    body: Result<Json<CommitContextPatchCommand>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ContextCommitView>)> {
    let Json(command) = json_body(body)?;
    if command.patch.aggregate_id != context_id || command.patch.lineage_key != branch_id {
        return Err(ApiError::bad_request(
            "context_path_mismatch",
            "patch aggregate does not match request path",
        ));
    }
    let result = state.context_service.commit_context_patch(command).await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn get_context_at_commit(
    State(state): State<AppState>,
    Path(commit_id): Path<String>,
) -> ApiResult<Json<WorkingContextView>> {
    Ok(Json(
        state
            .context_service
            .get_context_at_commit(&commit_id)
            .await?,
    ))
}

async fn create_snapshot(
    State(state): State<AppState>,
    Path(commit_id): Path<String>,
    body: Result<Json<SnapshotBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<VersionSnapshotView>)> {
    let Json(body) = json_body(body)?;
    let result = state
        .context_service
        .create_version_snapshot(CreateVersionSnapshotCommand {
            commit_id,
            retention_until: body.retention_until,
            pinned: body.pinned,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> ApiResult<Json<T>> {
    body.map_err(|error| ApiError::bad_request("invalid_json_body", error.body_text()))
}
