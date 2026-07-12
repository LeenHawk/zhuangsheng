use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{post, put},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::conversation::{
        RegenerateConversationCandidateCommand, RegenerateConversationCandidateResult,
        SelectConversationCandidateCommand,
    },
    conversation::{ConversationRunSpec, ConversationSelectionView},
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectCandidateBody {
    selected_run_id: String,
    expected_conversation_head_commit_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegenerateCandidateBody {
    expected_user_commit_id: String,
    run: ConversationRunSpec,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/conversation-turns/{turn_id}/selection",
            put(select_candidate),
        )
        .route(
            "/v1/conversation-turns/{turn_id}/candidates",
            post(regenerate_candidate),
        )
}

async fn select_candidate(
    State(state): State<AppState>,
    Path(turn_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<SelectCandidateBody>, JsonRejection>,
) -> ApiResult<Json<ConversationSelectionView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .conversation_service
            .select_candidate(SelectConversationCandidateCommand {
                turn_id,
                selected_run_id: body.selected_run_id,
                expected_conversation_head_commit_id: body.expected_conversation_head_commit_id,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn regenerate_candidate(
    State(state): State<AppState>,
    Path(turn_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<RegenerateCandidateBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<RegenerateConversationCandidateResult>)> {
    let body = json_body(body)?;
    let result = state
        .conversation_service
        .regenerate_candidate(RegenerateConversationCandidateCommand {
            turn_id,
            expected_user_commit_id: body.expected_user_commit_id,
            run: body.run,
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(result)))
}
