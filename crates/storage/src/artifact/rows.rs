use sea_orm::QueryResult;
use zhuangsheng_core::artifact::{ArtifactStagingStatus, ArtifactStagingView};

use crate::{StorageError, StorageResult};

pub(super) fn staging_view(row: &QueryResult) -> StorageResult<ArtifactStagingView> {
    let status = match row.try_get::<String>("", "status")?.as_str() {
        "uploading" => ArtifactStagingStatus::Uploading,
        "staged" => ArtifactStagingStatus::Staged,
        "validated" => ArtifactStagingStatus::Validated,
        "quarantined" => ArtifactStagingStatus::Quarantined,
        "deleting" => ArtifactStagingStatus::Deleting,
        "deleted" => ArtifactStagingStatus::Deleted,
        "committed" => ArtifactStagingStatus::Committed,
        _ => {
            return Err(StorageError::Integrity(
                "artifact staging status is invalid".into(),
            ));
        }
    };
    let lifecycle_generation = u64::try_from(row.try_get::<i64>("", "lifecycle_generation")?)
        .map_err(|_| StorageError::Integrity("artifact lifecycle generation is invalid".into()))?;
    let byte_size = row
        .try_get::<Option<i64>>("", "byte_size")?
        .map(u64::try_from)
        .transpose()
        .map_err(|_| StorageError::Integrity("artifact byte size is invalid".into()))?;
    Ok(ArtifactStagingView {
        staging_id: row.try_get("", "id")?,
        status,
        lifecycle_generation,
        byte_size,
        content_hash: row.try_get("", "content_hash")?,
        validated_media_type: row.try_get("", "validated_media_type")?,
    })
}

pub(super) const VIEW_COLUMNS: &str =
    "id, status, lifecycle_generation, byte_size, content_hash, validated_media_type";
