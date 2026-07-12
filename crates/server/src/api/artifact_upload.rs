use axum::{
    Json,
    extract::{Multipart, State, multipart::MultipartRejection},
    http::StatusCode,
};
use serde::Deserialize;
use zhuangsheng_core::{
    application::artifact::{CompleteArtifactStagingCommand, CreateArtifactStagingCommand},
    artifact::{ArtifactMetadataDraft, ArtifactStagingStatus, ArtifactStagingView},
};

use super::{
    AppState,
    error::{ApiError, ApiResult},
};

const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 16 * 1024;
pub(super) const MAX_UPLOAD_BODY_BYTES: usize = MAX_ARTIFACT_BYTES + MAX_METADATA_BYTES + 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadMetadata {
    context_id: Option<String>,
    metadata_draft: ArtifactMetadataDraft,
    declared_media_type: Option<String>,
}

pub(super) async fn upload(
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> ApiResult<(StatusCode, Json<ArtifactStagingView>)> {
    let mut multipart = multipart
        .map_err(|_| ApiError::bad_request("invalid_multipart", "invalid multipart upload"))?;
    let mut metadata = None;
    let mut completed = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad_request("invalid_multipart", "invalid multipart upload"))?
    {
        match field.name() {
            Some("metadata") if metadata.is_none() && completed.is_none() => {
                let bytes = read_field(&mut field, MAX_METADATA_BYTES).await?;
                metadata = Some(serde_json::from_slice::<UploadMetadata>(&bytes).map_err(
                    |_| {
                        ApiError::bad_request(
                            "invalid_artifact_metadata",
                            "invalid artifact metadata",
                        )
                    },
                )?);
            }
            Some("object") if metadata.is_some() && completed.is_none() => {
                completed = Some(
                    complete(&state, metadata.take().expect("checked above"), &mut field).await?,
                );
            }
            _ => {
                return Err(ApiError::bad_request(
                    "invalid_multipart_fields",
                    "expected metadata followed by one object field",
                ));
            }
        }
    }
    completed
        .map(|view| (StatusCode::CREATED, Json(view)))
        .ok_or_else(|| ApiError::bad_request("incomplete_multipart", "artifact object is missing"))
}

async fn complete(
    state: &AppState,
    input: UploadMetadata,
    field: &mut axum::extract::multipart::Field<'_>,
) -> ApiResult<ArtifactStagingView> {
    let staging = state
        .artifact_service
        .create_artifact_staging(CreateArtifactStagingCommand {
            context_id: input.context_id,
            node_attempt_id: None,
            tool_call_id: None,
            metadata_draft: input.metadata_draft,
            declared_media_type: input.declared_media_type,
        })
        .await?;
    let bytes = read_field(field, MAX_ARTIFACT_BYTES).await?;
    let view = state
        .artifact_service
        .complete_artifact_staging(CompleteArtifactStagingCommand {
            staging_id: staging.staging_id,
            expected_lifecycle_generation: staging.lifecycle_generation,
            bytes,
        })
        .await?;
    if view.status != ArtifactStagingStatus::Validated {
        return Err(ApiError::unprocessable(
            "artifact_quarantined",
            "artifact content failed validation",
        ));
    }
    Ok(view)
}

async fn read_field(
    field: &mut axum::extract::multipart::Field<'_>,
    limit: usize,
) -> ApiResult<Vec<u8>> {
    let mut output = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| ApiError::bad_request("invalid_multipart", "invalid multipart field"))?
    {
        if output.len().saturating_add(chunk.len()) > limit {
            return Err(ApiError::bad_request(
                "artifact_upload_too_large",
                "artifact upload exceeds the configured limit",
            ));
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}
