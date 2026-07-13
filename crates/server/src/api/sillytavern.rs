use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::Deserialize;
use serde_json::Value;
use zhuangsheng_core::{
    application::sillytavern::{
        ApplySillyTavernImportCommand, PreviewSillyTavernImportCommand, SillyTavernImportResult,
        apply_sillytavern_import, preview_sillytavern_import,
    },
    compatibility::sillytavern::SillyTavernImportPreview,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewBody {
    document: Value,
    source_name: Option<String>,
    target_preset_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyBody {
    document: Value,
    source_name: Option<String>,
    target_preset_id: Option<String>,
    expected_head_version_id: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/compatibility/sillytavern/preview", post(preview))
        .route("/v1/compatibility/sillytavern/import", post(apply))
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
}

async fn preview(
    State(state): State<AppState>,
    body: Result<Json<PreviewBody>, JsonRejection>,
) -> ApiResult<Json<SillyTavernImportPreview>> {
    let body = json_body(body)?;
    Ok(Json(
        preview_sillytavern_import(
            state.preset_service.as_ref(),
            PreviewSillyTavernImportCommand {
                document: body.document,
                source_name: body.source_name,
                target_preset_id: body.target_preset_id,
            },
        )
        .await?,
    ))
}

async fn apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ApplyBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<SillyTavernImportResult>)> {
    let body = json_body(body)?;
    Ok((
        StatusCode::CREATED,
        Json(
            apply_sillytavern_import(
                state.preset_service.as_ref(),
                ApplySillyTavernImportCommand {
                    document: body.document,
                    source_name: body.source_name,
                    target_preset_id: body.target_preset_id,
                    expected_head_version_id: body.expected_head_version_id,
                    idempotency_key: required_header(&headers, "idempotency-key")?,
                },
            )
            .await?,
        ),
    ))
}
