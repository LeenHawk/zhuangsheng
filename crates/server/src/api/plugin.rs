use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::application::plugin::{
    ActivatePluginCandidateCommand, ConfigurePluginCommand, InspectGitPluginCommand,
    PluginCandidateView, PluginEntrypointView, PluginInstallationView, PluginPermission,
    PluginUpdatePolicy, RollbackPluginCommand,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivateBody {
    expected_active_version_id: Option<String>,
    approved_permissions: Vec<PluginPermission>,
    update_policy: PluginUpdatePolicy,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigureBody {
    enabled: bool,
    update_policy: PluginUpdatePolicy,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RollbackBody {
    target_version_id: String,
    expected_active_version_id: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/plugins", get(list).post(inspect))
        .route(
            "/v1/plugins/candidates/{candidate_id}/activate",
            post(activate),
        )
        .route("/v1/plugins/{plugin_id}/configure", post(configure))
        .route("/v1/plugins/{plugin_id}/check-update", post(check_update))
        .route("/v1/plugins/{plugin_id}/rollback", post(rollback))
        .route("/v1/plugins/{plugin_id}/entrypoint", get(entrypoint))
}

async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<PluginInstallationView>>> {
    Ok(Json(state.plugin_service.list_installations().await?))
}

async fn inspect(
    State(state): State<AppState>,
    body: Result<Json<InspectGitPluginCommand>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<PluginCandidateView>)> {
    let candidate = state
        .plugin_service
        .inspect_git_source(json_body(body)?)
        .await?;
    Ok((StatusCode::CREATED, Json(candidate)))
}

async fn activate(
    State(state): State<AppState>,
    Path(candidate_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<ActivateBody>, JsonRejection>,
) -> ApiResult<Json<PluginInstallationView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .plugin_service
            .activate_candidate(ActivatePluginCandidateCommand {
                candidate_id,
                expected_active_version_id: body.expected_active_version_id,
                approved_permissions: body.approved_permissions,
                update_policy: body.update_policy,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn configure(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<ConfigureBody>, JsonRejection>,
) -> ApiResult<Json<PluginInstallationView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .plugin_service
            .configure_plugin(ConfigurePluginCommand {
                plugin_id,
                enabled: body.enabled,
                update_policy: body.update_policy,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn check_update(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
) -> ApiResult<Json<Option<PluginCandidateView>>> {
    Ok(Json(state.plugin_service.check_update(&plugin_id).await?))
}

async fn rollback(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<RollbackBody>, JsonRejection>,
) -> ApiResult<Json<PluginInstallationView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .plugin_service
            .rollback_plugin(RollbackPluginCommand {
                plugin_id,
                target_version_id: body.target_version_id,
                expected_active_version_id: body.expected_active_version_id,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn entrypoint(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
) -> ApiResult<Json<PluginEntrypointView>> {
    Ok(Json(state.plugin_service.get_entrypoint(&plugin_id).await?))
}
