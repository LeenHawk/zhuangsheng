use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::HeaderMap,
    routing::post,
};
use serde::Deserialize;
use serde_json::Value;
use ulid::Ulid;
use zhuangsheng_core::llm::{
    EffectResolutionActorKind, EffectResolutionKind, EffectResolutionView,
    ResolveEffectUnknownCommand,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveEffectBody {
    expected_effect_attempt_id: String,
    expected_run_control_epoch: u64,
    kind: EffectResolutionKind,
    decision: Value,
    result_object_id: Option<String>,
    evidence_object_id: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/effects/{effect_id}/resolution", post(resolve_effect))
}

async fn resolve_effect(
    State(state): State<AppState>,
    Path(effect_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<ResolveEffectBody>, JsonRejection>,
) -> ApiResult<Json<EffectResolutionView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .runtime_service
            .resolve_effect_unknown(ResolveEffectUnknownCommand {
                resolution_id: format!("effectresolution_{}", Ulid::new()),
                effect_id,
                expected_effect_attempt_id: body.expected_effect_attempt_id,
                expected_run_control_epoch: body.expected_run_control_epoch,
                command_idempotency_key: required_header(&headers, "idempotency-key")?,
                kind: body.kind,
                decision: body.decision,
                result_object_id: body.result_object_id,
                evidence_object_id: body.evidence_object_id,
                actor_kind: EffectResolutionActorKind::Human,
                actor_id: Some("local-user".into()),
            })
            .await?,
    ))
}
