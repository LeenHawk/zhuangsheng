use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::artifact::CommitArtifactStagingCommand,
    artifact::{ArtifactStagingView, ArtifactView},
};

use super::{
    AppState,
    artifact_upload::{MAX_UPLOAD_BODY_BYTES, upload},
    error::{ApiError, ApiResult},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitBody {
    expected_lifecycle_generation: u64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/artifacts/staging", post(upload))
        .route("/v1/artifacts/staging/{staging_id}", get(get_staging))
        .route("/v1/artifacts/staging/{staging_id}/commit", post(commit))
        .route("/v1/artifacts/{artifact_id}", get(get_artifact))
        .route(
            "/v1/artifacts/{artifact_id}/content",
            get(download_artifact),
        )
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES))
}

async fn get_staging(
    State(state): State<AppState>,
    Path(staging_id): Path<String>,
) -> ApiResult<Json<ArtifactStagingView>> {
    Ok(Json(
        state
            .artifact_service
            .get_artifact_staging(&staging_id)
            .await?,
    ))
}

async fn commit(
    State(state): State<AppState>,
    Path(staging_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<CommitBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<ArtifactView>)> {
    let Json(body) =
        body.map_err(|error| ApiError::bad_request("invalid_json_body", error.body_text()))?;
    let view = state
        .artifact_service
        .commit_artifact_staging(CommitArtifactStagingCommand {
            staging_id,
            expected_lifecycle_generation: body.expected_lifecycle_generation,
            idempotency_key: idempotency_key(&headers)?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

async fn get_artifact(
    State(state): State<AppState>,
    Path(artifact_id): Path<String>,
) -> ApiResult<Json<ArtifactView>> {
    Ok(Json(
        state.artifact_service.get_artifact(&artifact_id).await?,
    ))
}

async fn download_artifact(
    State(state): State<AppState>,
    Path(artifact_id): Path<String>,
) -> ApiResult<Response> {
    let download = state
        .artifact_service
        .download_artifact(&artifact_id)
        .await?;
    let metadata = &download.artifact.metadata;
    let filename = safe_filename(metadata.name.as_deref());
    let mut response = Response::new(Body::from(download.bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&metadata.content.media_type)
            .map_err(|_| ApiError::unprocessable("invalid_media_type", "invalid media type"))?,
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&metadata.content.byte_size.to_string())
            .map_err(|_| ApiError::unprocessable("invalid_size", "invalid artifact size"))?,
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| ApiError::unprocessable("invalid_filename", "invalid artifact name"))?,
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

fn safe_filename(name: Option<&str>) -> &str {
    name.filter(|name| {
        !name.is_empty()
            && name.is_ascii()
            && name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b' ')
            })
    })
    .unwrap_or("artifact")
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
