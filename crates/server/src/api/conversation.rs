use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::conversation::CreateConversationCommand, conversation::ConversationView,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
struct CreateConversationBody {
    title: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/conversations", post(create_conversation))
        .route("/v1/conversations/{conversation_id}", get(get_conversation))
}

async fn create_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CreateConversationBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ConversationView>)> {
    let body = json_body(body)?;
    let conversation = state
        .conversation_service
        .create_conversation(CreateConversationCommand {
            title: body.title,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(conversation)))
}

async fn get_conversation(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
) -> ApiResult<Json<ConversationView>> {
    Ok(Json(
        state
            .conversation_service
            .get_conversation(&conversation_id)
            .await?,
    ))
}
