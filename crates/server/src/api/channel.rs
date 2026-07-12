use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::channel::{
        ChannelModelDiscoveryView, ChannelView, CreateChannelCommand, DiscoverChannelModelsCommand,
        PublishChannelRevisionCommand,
    },
    llm::{LlmChannelRevision, LlmChannelRevisionSpec, OperationKey},
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
struct CreateChannelBody {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishRevisionBody {
    expected_head_revision_id: Option<String>,
    spec: LlmChannelRevisionSpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoverModelsBody {
    revision_id: Option<String>,
    operation_key: Option<OperationKey>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/channels", post(create_channel).get(list_channels))
        .route("/v1/channels/{channel_id}", get(get_channel))
        .route(
            "/v1/channels/{channel_id}/revisions",
            post(publish_revision),
        )
        .route("/v1/channels/{channel_id}/head", get(get_head))
        .route(
            "/v1/channels/{channel_id}/model-discovery",
            post(discover_models),
        )
        .route("/v1/channel-revisions/{revision_id}", get(get_revision))
}

async fn discover_models(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    body: Result<Json<DiscoverModelsBody>, JsonRejection>,
) -> ApiResult<Json<ChannelModelDiscoveryView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .model_discovery_service
            .discover_models(DiscoverChannelModelsCommand {
                channel_id,
                revision_id: body.revision_id,
                operation_key: body.operation_key,
            })
            .await?,
    ))
}

async fn create_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CreateChannelBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ChannelView>)> {
    let body = json_body(body)?;
    let view = state
        .channel_service
        .create_channel(CreateChannelCommand {
            name: body.name,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn list_channels(State(state): State<AppState>) -> ApiResult<Json<Vec<ChannelView>>> {
    Ok(Json(state.channel_service.list_channels().await?))
}

async fn get_channel(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> ApiResult<Json<ChannelView>> {
    Ok(Json(state.channel_service.get_channel(&channel_id).await?))
}

async fn publish_revision(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<PublishRevisionBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<LlmChannelRevision>)> {
    let body = json_body(body)?;
    let revision = state
        .channel_service
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id,
            expected_head_revision_id: body.expected_head_revision_id,
            spec: body.spec,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(revision)))
}

async fn get_head(
    State(state): State<AppState>,
    Path(channel_id): Path<String>,
) -> ApiResult<Json<LlmChannelRevision>> {
    Ok(Json(
        state
            .channel_service
            .get_channel_head_revision(&channel_id)
            .await?,
    ))
}

async fn get_revision(
    State(state): State<AppState>,
    Path(revision_id): Path<String>,
) -> ApiResult<Json<LlmChannelRevision>> {
    Ok(Json(
        state
            .channel_service
            .get_channel_revision(&revision_id)
            .await?,
    ))
}
