use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::application::context::{
    CommitContextPatchCommand, ContextCommitView, ContextDiffView, CreateVersionSnapshotCommand,
    VersionSnapshotView, WorkingContextView,
};
use zhuangsheng_core::runtime::{ContextBranchView, ForkContextCommand};
use zhuangsheng_core::state::{ActorKind, ActorRef};

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkContextBody {
    source_branch_id: String,
    from_commit_id: String,
    expected_source_head: Option<String>,
}

#[derive(Deserialize)]
struct DiffQuery {
    from: String,
    to: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/contexts/{context_id}/branches/{branch_id}",
            get(get_context),
        )
        .route(
            "/v1/contexts/{context_id}/branches",
            get(list_context_branches).post(fork_context),
        )
        .route(
            "/v1/contexts/{context_id}/commits",
            get(list_context_commits),
        )
        .route("/v1/contexts/{context_id}/diff", get(diff_context))
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

async fn list_context_branches(
    State(state): State<AppState>,
    Path(context_id): Path<String>,
) -> ApiResult<Json<Vec<ContextBranchView>>> {
    Ok(Json(
        state
            .context_service
            .list_context_branches(&context_id)
            .await?,
    ))
}

async fn list_context_commits(
    State(state): State<AppState>,
    Path(context_id): Path<String>,
) -> ApiResult<Json<Vec<ContextCommitView>>> {
    Ok(Json(
        state
            .context_service
            .list_context_commits(&context_id)
            .await?,
    ))
}

async fn diff_context(
    State(state): State<AppState>,
    Path(context_id): Path<String>,
    Query(query): Query<DiffQuery>,
) -> ApiResult<Json<ContextDiffView>> {
    Ok(Json(
        state
            .context_service
            .diff_context_commits(&context_id, &query.from, &query.to)
            .await?,
    ))
}

async fn fork_context(
    State(state): State<AppState>,
    Path(context_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<ForkContextBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ContextBranchView>)> {
    let Json(body) = json_body(body)?;
    let branch = state
        .runtime_service
        .fork_context(ForkContextCommand {
            context_id,
            source_branch_id: body.source_branch_id,
            from_commit_id: body.from_commit_id,
            expected_source_head: body.expected_source_head,
            idempotency_key: super::graph::required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(branch)))
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
    let Json(mut command) = json_body(body)?;
    if command.patch.aggregate_id != context_id || command.patch.lineage_key != branch_id {
        return Err(ApiError::bad_request(
            "context_path_mismatch",
            "patch aggregate does not match request path",
        ));
    }
    command.patch.author = local_actor();
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

fn local_actor() -> ActorRef {
    ActorRef {
        kind: ActorKind::User,
        id: Some("local-user".into()),
    }
}
