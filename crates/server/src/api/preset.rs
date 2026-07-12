use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use zhuangsheng_core::{
    application::preset::{
        ContextPresetPreviewView, ContextPresetView, CreateContextPresetCommand,
        PreviewContextPresetCommand, PublishContextPresetVersionCommand,
    },
    llm::context::{
        ContextAssemblySpec, ContextBudgetInput, ContextPresetVersion, ResolvedContextBinding,
    },
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
struct CreatePresetBody {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishVersionBody {
    expected_head_version_id: Option<String>,
    spec: ContextAssemblySpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewBody {
    version_id: Option<String>,
    #[serde(default)]
    node_input: Value,
    #[serde(default)]
    sample_bindings: BTreeMap<String, ResolvedContextBinding>,
    budget: ContextBudgetInput,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/context-presets", post(create_preset).get(list_presets))
        .route("/v1/context-presets/{preset_id}", get(get_preset))
        .route(
            "/v1/context-presets/{preset_id}/revisions",
            post(publish_version),
        )
        .route("/v1/context-presets/{preset_id}/head", get(get_head))
        .route("/v1/context-presets/{preset_id}/preview", post(preview))
        .route("/v1/context-preset-versions/{version_id}", get(get_version))
}

async fn preview(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
    body: Result<Json<PreviewBody>, JsonRejection>,
) -> ApiResult<Json<ContextPresetPreviewView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .preset_service
            .preview_context_preset(PreviewContextPresetCommand {
                preset_id,
                version_id: body.version_id,
                node_input: body.node_input,
                sample_bindings: body.sample_bindings,
                budget: body.budget,
            })
            .await?,
    ))
}

async fn create_preset(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CreatePresetBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ContextPresetView>)> {
    let body = json_body(body)?;
    let view = state
        .preset_service
        .create_context_preset(CreateContextPresetCommand {
            name: body.name,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn list_presets(State(state): State<AppState>) -> ApiResult<Json<Vec<ContextPresetView>>> {
    Ok(Json(state.preset_service.list_context_presets().await?))
}

async fn get_preset(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
) -> ApiResult<Json<ContextPresetView>> {
    Ok(Json(
        state.preset_service.get_context_preset(&preset_id).await?,
    ))
}

async fn publish_version(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<PublishVersionBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ContextPresetVersion>)> {
    let body = json_body(body)?;
    let version = state
        .preset_service
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id,
            expected_head_version_id: body.expected_head_version_id,
            spec: body.spec,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(version)))
}

async fn get_head(
    State(state): State<AppState>,
    Path(preset_id): Path<String>,
) -> ApiResult<Json<ContextPresetVersion>> {
    Ok(Json(
        state
            .preset_service
            .get_context_preset_head(&preset_id)
            .await?,
    ))
}

async fn get_version(
    State(state): State<AppState>,
    Path(version_id): Path<String>,
) -> ApiResult<Json<ContextPresetVersion>> {
    Ok(Json(
        state
            .preset_service
            .get_context_preset_version(&version_id)
            .await?,
    ))
}
