use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::graph::{CreateRolePlayTemplateCommand, GraphRevisionView},
    conversation::{RolePlayCompatibilityView, RolePlayGraphOptionView, RolePlaySettingsView},
    graph::{GenerationOptionsIr, ProviderExtensionsIr},
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRolePlayTemplateBody {
    name: String,
    channel_id: String,
    preset_id: String,
    generation: Option<GenerationOptionsIr>,
    extensions: Option<ProviderExtensionsIr>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/roleplay/templates", post(create_roleplay_template))
        .route("/v1/roleplay/graph-options", get(list_options))
        .route(
            "/v1/graph-revisions/{revision_id}/roleplay-compatibility",
            get(get_compatibility),
        )
        .route(
            "/v1/graph-revisions/{revision_id}/roleplay-settings",
            get(get_settings),
        )
}

async fn create_roleplay_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CreateRolePlayTemplateBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<GraphRevisionView>)> {
    let body = json_body(body)?;
    Ok((
        StatusCode::CREATED,
        Json(
            state
                .graph_service
                .create_roleplay_template(CreateRolePlayTemplateCommand {
                    name: body.name,
                    channel_id: body.channel_id,
                    preset_id: body.preset_id,
                    generation: body.generation,
                    extensions: body.extensions,
                    idempotency_key: required_header(&headers, "idempotency-key")?,
                })
                .await?,
        ),
    ))
}

async fn list_options(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<RolePlayGraphOptionView>>> {
    Ok(Json(
        state.graph_service.list_roleplay_graph_options().await?,
    ))
}

async fn get_compatibility(
    State(state): State<AppState>,
    Path(revision_id): Path<String>,
) -> ApiResult<Json<RolePlayCompatibilityView>> {
    Ok(Json(
        state
            .graph_service
            .get_roleplay_compatibility(&revision_id)
            .await?,
    ))
}

async fn get_settings(
    State(state): State<AppState>,
    Path(revision_id): Path<String>,
) -> ApiResult<Json<RolePlaySettingsView>> {
    Ok(Json(
        state
            .graph_service
            .get_roleplay_settings(&revision_id)
            .await?,
    ))
}
