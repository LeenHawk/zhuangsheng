use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::memory::{
        ApplyMemoryProposalCommand, DecideMemoryProposalCommand, MemoryProposalDecision,
        MemorySearchCommand, MemorySearchView, ProposeMemoryChangeCommand,
    },
    memory::{
        LongTermMemoryRecordView, MemoryChangeProposalView, MemoryProposalChangeInput,
        MemoryProposalStatus,
    },
    state::ActorRef,
};

use super::{
    AppState,
    error::{ApiError, ApiResult},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProposeBody {
    scope_id: String,
    memory_id: Option<String>,
    expected_head_commit_id: Option<String>,
    change: MemoryProposalChangeInput,
    reason: String,
    evidence_refs: Vec<String>,
    requested_by: ActorRef,
    schema_version: u32,
    policy_version: u32,
    origin_run_id: Option<String>,
    origin_node_instance_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecisionBody {
    expected_status: MemoryProposalStatus,
    decision: MemoryProposalDecision,
    actor: ActorRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyBody {
    expected_status: MemoryProposalStatus,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/memory-proposals", post(propose))
        .route("/v1/memory-proposals/{proposal_id}/decision", post(decide))
        .route("/v1/memory-proposals/{proposal_id}/apply", post(apply))
        .route("/v1/memories/{memory_id}", get(get_record))
        .route("/v1/memory-search", post(search))
}

async fn propose(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ProposeBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<MemoryChangeProposalView>)> {
    let Json(body) = json_body(body)?;
    let result = state
        .memory_service
        .propose_memory_change(ProposeMemoryChangeCommand {
            scope_id: body.scope_id,
            memory_id: body.memory_id,
            expected_head_commit_id: body.expected_head_commit_id,
            change: body.change,
            reason: body.reason,
            evidence_refs: body.evidence_refs,
            requested_by: body.requested_by,
            idempotency_key: idempotency_key(&headers)?,
            schema_version: body.schema_version,
            policy_version: body.policy_version,
            origin_run_id: body.origin_run_id,
            origin_node_instance_id: body.origin_node_instance_id,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn decide(
    State(state): State<AppState>,
    Path(proposal_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<DecisionBody>, JsonRejection>,
) -> ApiResult<Json<MemoryChangeProposalView>> {
    let Json(body) = json_body(body)?;
    Ok(Json(
        state
            .memory_service
            .decide_memory_proposal(DecideMemoryProposalCommand {
                proposal_id,
                expected_status: body.expected_status,
                decision: body.decision,
                actor: body.actor,
                idempotency_key: idempotency_key(&headers)?,
            })
            .await?,
    ))
}

async fn apply(
    State(state): State<AppState>,
    Path(proposal_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<ApplyBody>, JsonRejection>,
) -> ApiResult<Json<MemoryChangeProposalView>> {
    let Json(body) = json_body(body)?;
    Ok(Json(
        state
            .memory_service
            .apply_memory_proposal(ApplyMemoryProposalCommand {
                proposal_id,
                expected_status: body.expected_status,
                idempotency_key: idempotency_key(&headers)?,
            })
            .await?,
    ))
}

async fn get_record(
    State(state): State<AppState>,
    Path(memory_id): Path<String>,
) -> ApiResult<Json<LongTermMemoryRecordView>> {
    Ok(Json(
        state.memory_service.get_memory_record(&memory_id).await?,
    ))
}

async fn search(
    State(state): State<AppState>,
    body: Result<Json<MemorySearchCommand>, JsonRejection>,
) -> ApiResult<Json<MemorySearchView>> {
    let Json(command) = json_body(body)?;
    Ok(Json(state.memory_service.search_memory(command).await?))
}

fn idempotency_key(headers: &HeaderMap) -> ApiResult<String> {
    let value = headers
        .get("idempotency-key")
        .ok_or_else(|| ApiError::bad_request("missing_header", "missing idempotency-key"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid_header", "invalid idempotency-key"))?
        .trim();
    if value.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_header",
            "empty idempotency-key",
        ));
    }
    Ok(value.into())
}

fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> ApiResult<Json<T>> {
    body.map_err(|error| ApiError::bad_request("invalid_json_body", error.body_text()))
}
