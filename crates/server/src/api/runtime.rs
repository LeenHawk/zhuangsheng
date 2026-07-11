use std::{convert::Infallible, time::Duration};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header},
    response::{
        Response, Sse,
        sse::{Event as SseEvent, KeepAlive},
    },
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;
use zhuangsheng_core::runtime::{RunContextCommand, RunControlCommand, RunView, StartRunCommand};

use super::{
    AppState,
    error::{ApiError, ApiResult},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartRunBody {
    input: Value,
    context: RunContextCommand,
    deadline_at: Option<i64>,
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunControlBody {
    expected_epoch: u64,
    reason: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/graphs/{graph_revision_id}/runs", post(start_run))
        .route("/v1/runs/{run_id}", get(get_run))
        .route("/v1/runs/{run_id}/outputs", get(get_outputs))
        .route("/v1/runs/{run_id}/events", get(run_events))
        .route("/v1/runs/{run_id}/interrupt", post(interrupt_run))
        .route("/v1/runs/{run_id}/resume", post(resume_run))
        .route("/v1/runs/{run_id}/cancel", post(cancel_run))
        .route("/v1/values/{value_ref}", get(get_value))
}

async fn start_run(
    State(state): State<AppState>,
    Path(graph_revision_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<StartRunBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<RunView>)> {
    let Json(body) =
        body.map_err(|error| ApiError::bad_request("invalid_json_body", error.body_text()))?;
    let idempotency_key = headers
        .get("idempotency-key")
        .ok_or_else(|| ApiError::bad_request("missing_header", "missing idempotency-key"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid_header", "invalid idempotency-key"))?
        .trim()
        .to_owned();
    if idempotency_key.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_header",
            "empty idempotency-key",
        ));
    }
    let run = state
        .runtime_service
        .start_run(StartRunCommand {
            graph_revision_id,
            input: body.input,
            context: body.context,
            deadline_at: body.deadline_at,
            idempotency_key,
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(run)))
}

async fn get_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> ApiResult<Json<RunView>> {
    Ok(Json(state.runtime_service.get_run(&run_id).await?))
}

async fn get_outputs(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> ApiResult<Json<zhuangsheng_core::runtime::RunOutputsView>> {
    Ok(Json(state.runtime_service.get_run_outputs(&run_id).await?))
}

async fn run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> ApiResult<Sse<impl futures_core::Stream<Item = Result<SseEvent, Infallible>>>> {
    state.runtime_service.get_run(&run_id).await?;
    let header_cursor = headers
        .get("last-event-id")
        .map(|value| {
            value
                .to_str()
                .map_err(|_| {
                    ApiError::bad_request("invalid_event_cursor", "invalid Last-Event-ID")
                })?
                .parse::<u64>()
                .map_err(|_| ApiError::bad_request("invalid_event_cursor", "invalid Last-Event-ID"))
        })
        .transpose()?;
    let mut cursor = header_cursor.or(query.after).unwrap_or(0);
    let service = state.runtime_service;
    let stream = async_stream::stream! {
        loop {
            match service.list_run_events(&run_id, cursor, 100).await {
                Ok(events) if events.is_empty() => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Ok(events) => {
                    for event in events {
                        cursor = event.durable_seq;
                        let data = serde_json::to_string(&event)
                            .unwrap_or_else(|_| "{\"error\":\"event_serialization_failed\"}".into());
                        yield Ok(SseEvent::default()
                            .id(event.durable_seq.to_string())
                            .event(event.event_type)
                            .data(data));
                    }
                }
                Err(_) => break,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat"),
    ))
}

async fn get_value(
    State(state): State<AppState>,
    Path(value_ref): Path<String>,
) -> ApiResult<Response> {
    let bytes = state
        .runtime_service
        .load_json_value_bytes(&value_ref)
        .await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .body(Body::from(bytes))
        .map_err(|_| ApiError::bad_request("response_build_failed", "could not build response"))
}

async fn interrupt_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<RunControlBody>, JsonRejection>,
) -> ApiResult<Json<RunView>> {
    let command = control_command(run_id, &headers, body)?;
    Ok(Json(
        state.runtime_service.request_interrupt(command).await?,
    ))
}

async fn resume_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<RunControlBody>, JsonRejection>,
) -> ApiResult<Json<RunView>> {
    let command = control_command(run_id, &headers, body)?;
    Ok(Json(
        state.runtime_service.resume_interrupted(command).await?,
    ))
}

async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<RunControlBody>, JsonRejection>,
) -> ApiResult<Json<RunView>> {
    let command = control_command(run_id, &headers, body)?;
    Ok(Json(state.runtime_service.request_cancel(command).await?))
}

fn control_command(
    run_id: String,
    headers: &HeaderMap,
    body: Result<Json<RunControlBody>, JsonRejection>,
) -> ApiResult<RunControlCommand> {
    let Json(body) =
        body.map_err(|error| ApiError::bad_request("invalid_json_body", error.body_text()))?;
    let idempotency_key = headers
        .get("idempotency-key")
        .ok_or_else(|| ApiError::bad_request("missing_header", "missing idempotency-key"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid_header", "invalid idempotency-key"))?
        .trim()
        .to_owned();
    if idempotency_key.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_header",
            "empty idempotency-key",
        ));
    }
    Ok(RunControlCommand {
        run_id,
        expected_epoch: body.expected_epoch,
        idempotency_key,
        reason: body.reason,
    })
}
