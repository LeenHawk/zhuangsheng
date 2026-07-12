use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::graph::{
        ApplyGraphCommand, CreateGraphCommand, CreateGraphResult, GraphDraftView,
        GraphRevisionView, GraphView, UpdateGraphDraftCommand,
    },
    graph::GraphDraft,
};

use super::{
    AppState,
    error::{ApiError, ApiResult},
};

const IDEMPOTENCY_KEY: &str = "idempotency-key";
const IF_MATCH: &str = "if-match";

#[derive(Deserialize)]
struct CreateGraphBody {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyGraphBody {
    operation_taxonomy_version: u32,
    adapter_decoder_version: u32,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/graphs", post(create_graph).get(list_graphs))
        .route(
            "/v1/graphs/{graph_id}/draft",
            get(get_draft).put(update_draft),
        )
        .route("/v1/graphs/{graph_id}/apply", post(apply_graph))
        .route(
            "/v1/graphs/{graph_id}/revisions/{revision_id}",
            get(get_nested_revision),
        )
        .route("/v1/graph-revisions/{revision_id}", get(get_revision))
}

async fn create_graph(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CreateGraphBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<CreateGraphResult>)> {
    let body = json_body(body)?;
    let result = state
        .graph_service
        .create_graph(CreateGraphCommand {
            name: body.name,
            idempotency_key: required_header(&headers, IDEMPOTENCY_KEY)?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn list_graphs(State(state): State<AppState>) -> ApiResult<Json<Vec<GraphView>>> {
    Ok(Json(state.graph_service.list_graphs().await?))
}

async fn get_draft(
    State(state): State<AppState>,
    Path(graph_id): Path<String>,
) -> ApiResult<Json<GraphDraftView>> {
    Ok(Json(state.graph_service.get_graph_draft(&graph_id).await?))
}

async fn update_draft(
    State(state): State<AppState>,
    Path(graph_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<GraphDraft>, JsonRejection>,
) -> ApiResult<Json<GraphDraftView>> {
    let result = state
        .graph_service
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id,
            expected_revision_token: revision_token(&headers)?,
            document: json_body(body)?,
            idempotency_key: required_header(&headers, IDEMPOTENCY_KEY)?,
        })
        .await?;
    Ok(Json(result))
}

async fn apply_graph(
    State(state): State<AppState>,
    Path(graph_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<ApplyGraphBody>, JsonRejection>,
) -> ApiResult<Json<GraphRevisionView>> {
    let body = json_body(body)?;
    let result = state
        .graph_service
        .apply_graph(ApplyGraphCommand {
            graph_id,
            expected_revision_token: revision_token(&headers)?,
            operation_taxonomy_version: body.operation_taxonomy_version,
            adapter_decoder_version: body.adapter_decoder_version,
            idempotency_key: required_header(&headers, IDEMPOTENCY_KEY)?,
        })
        .await?;
    Ok(Json(result))
}

async fn get_revision(
    State(state): State<AppState>,
    Path(revision_id): Path<String>,
) -> ApiResult<Json<GraphRevisionView>> {
    Ok(Json(
        state.graph_service.get_graph_revision(&revision_id).await?,
    ))
}

async fn get_nested_revision(
    State(state): State<AppState>,
    Path((graph_id, revision_id)): Path<(String, String)>,
) -> ApiResult<Json<GraphRevisionView>> {
    Ok(Json(
        state
            .graph_service
            .get_graph_revision_for_graph(&graph_id, &revision_id)
            .await?,
    ))
}

pub(super) fn json_body<T>(body: Result<Json<T>, JsonRejection>) -> ApiResult<T> {
    body.map(|Json(value)| value)
        .map_err(|error| ApiError::bad_request("invalid_json_body", error.body_text()))
}

pub(super) fn required_header(headers: &HeaderMap, name: &'static str) -> ApiResult<String> {
    let value = headers
        .get(name)
        .ok_or_else(|| ApiError::bad_request("missing_header", format!("missing {name}")))?
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid_header", format!("invalid {name}")))?
        .trim();
    if value.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_header",
            format!("empty {name}"),
        ));
    }
    Ok(value.into())
}

fn revision_token(headers: &HeaderMap) -> ApiResult<String> {
    let value = required_header(headers, IF_MATCH)?;
    Ok(value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(&value)
        .to_owned())
}
