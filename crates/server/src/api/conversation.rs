use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::conversation::{
        CreateConversationCommand, SubmitConversationTurnCommand, SubmitConversationTurnResult,
        UpdateConversationRunProfileCommand,
    },
    conversation::{ConversationRunProfile, ConversationRunSpec, ConversationView},
    llm::ir::LlmContentPartIr,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateConversationBody {
    title: Option<String>,
    default_run: Option<ConversationRunSpec>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRunProfileBody {
    expected_revision_no: u64,
    run: ConversationRunSpec,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitTurnBody {
    expected_head_commit_id: String,
    user_content: Vec<LlmContentPartIr>,
    run: ConversationRunSpec,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/conversations", post(create_conversation))
        .route("/v1/conversations/{conversation_id}", get(get_conversation))
        .route(
            "/v1/conversations/{conversation_id}/run-profile",
            put(update_run_profile),
        )
        .route(
            "/v1/conversations/{conversation_id}/turns",
            post(submit_turn),
        )
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
            default_run: body.default_run,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(conversation)))
}

async fn update_run_profile(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<UpdateRunProfileBody>, JsonRejection>,
) -> ApiResult<Json<ConversationRunProfile>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .conversation_service
            .update_run_profile(UpdateConversationRunProfileCommand {
                conversation_id,
                expected_revision_no: body.expected_revision_no,
                run: body.run,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn submit_turn(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<SubmitTurnBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<SubmitConversationTurnResult>)> {
    let body = json_body(body)?;
    let result = state
        .conversation_service
        .submit_turn(SubmitConversationTurnCommand {
            conversation_id,
            expected_head_commit_id: body.expected_head_commit_id,
            user_content: body.user_content,
            run: body.run,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(result)))
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
