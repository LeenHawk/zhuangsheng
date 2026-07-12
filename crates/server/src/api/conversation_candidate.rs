use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::conversation::{
        CandidateProjectionResolution, RegenerateConversationCandidateCommand,
        RegenerateConversationCandidateResult, ResolveCandidateProjectionCommand,
        ResolveCandidateProjectionResult, SelectConversationCandidateCommand,
    },
    conversation::{ConversationRunSpec, ConversationSelectionView, ConversationTurnDetailView},
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveProjectionBody {
    expected_current_branch_head: String,
    resolution: CandidateProjectionResolution,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/turns/{turn_id}/candidates", get(get_candidates))
        .route("/v1/turns/{turn_id}/selection", put(select_candidate))
        .route(
            "/v1/turns/{turn_id}/regenerations",
            post(regenerate_candidate),
        )
        .route(
            "/v1/turns/{turn_id}/candidates/{run_id}/projection-resolution",
            post(resolve_projection),
        )
}

async fn get_candidates(
    State(state): State<AppState>,
    Path(turn_id): Path<String>,
) -> ApiResult<Json<ConversationTurnDetailView>> {
    Ok(Json(
        state
            .conversation_service
            .get_turn_candidates(&turn_id)
            .await?,
    ))
}

async fn resolve_projection(
    State(state): State<AppState>,
    Path((turn_id, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Result<Json<ResolveProjectionBody>, JsonRejection>,
) -> ApiResult<Json<ResolveCandidateProjectionResult>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .conversation_service
            .resolve_candidate_projection(ResolveCandidateProjectionCommand {
                turn_id,
                run_id,
                expected_current_branch_head: body.expected_current_branch_head,
                resolution: body.resolution,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
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
