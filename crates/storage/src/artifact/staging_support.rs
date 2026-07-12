use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::artifact::CompleteArtifactStagingCommand, artifact::ArtifactStagingView,
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::rows::{VIEW_COLUMNS, staging_view};

pub(super) struct StagingOwner {
    pub status: String,
    pub generation: u64,
    pub lease_until: Option<i64>,
    pub declared_media_type: Option<String>,
    pub byte_size: Option<i64>,
    pub content_hash: Option<String>,
    pub validated_media_type: Option<String>,
    pub validated_object_id: Option<String>,
    pub node_attempt_id: Option<String>,
    pub node_attempt_status: Option<String>,
}

pub(super) async fn load_owner<C: ConnectionTrait>(
    connection: &C,
    id: &str,
) -> StorageResult<StagingOwner> {
    let row = connection.query_one_raw(sql(
        "SELECT s.status, s.lifecycle_generation, s.lease_until, s.expected_media_type, s.byte_size, s.content_hash, s.validated_media_type, s.validated_content_object_id, s.metadata_draft_digest, s.node_attempt_id, metadata.content_hash AS metadata_hash, metadata.lifecycle AS metadata_lifecycle, attempt.status AS node_attempt_status FROM artifact_staging s LEFT JOIN content_objects metadata ON metadata.id = s.metadata_draft_object_id LEFT JOIN node_attempts attempt ON attempt.id = s.node_attempt_id WHERE s.id = ?",
        vec![id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "artifact_staging", id: id.into() })?;
    let metadata_digest: String = row.try_get("", "metadata_draft_digest")?;
    let metadata_hash: Option<String> = row.try_get("", "metadata_hash")?;
    if row
        .try_get::<Option<String>>("", "metadata_lifecycle")?
        .as_deref()
        != Some("live")
        || metadata_hash.as_deref() != Some(metadata_digest.as_str())
    {
        return Err(StorageError::Integrity(
            "artifact metadata draft failed integrity validation".into(),
        ));
    }
    Ok(StagingOwner {
        status: row.try_get("", "status")?,
        generation: u64::try_from(row.try_get::<i64>("", "lifecycle_generation")?)
            .map_err(|_| StorageError::Integrity("artifact generation is invalid".into()))?,
        lease_until: row.try_get("", "lease_until")?,
        declared_media_type: row.try_get("", "expected_media_type")?,
        byte_size: row.try_get("", "byte_size")?,
        content_hash: row.try_get("", "content_hash")?,
        validated_media_type: row.try_get("", "validated_media_type")?,
        validated_object_id: row.try_get("", "validated_content_object_id")?,
        node_attempt_id: row.try_get("", "node_attempt_id")?,
        node_attempt_status: row.try_get("", "node_attempt_status")?,
    })
}

pub(super) async fn validate_replay_content<C: ConnectionTrait>(
    connection: &C,
    owner: &StagingOwner,
    staging_id: &str,
    expected: &[u8],
) -> StorageResult<()> {
    let object_id = owner
        .validated_object_id
        .as_deref()
        .ok_or_else(|| StorageError::Integrity("validated artifact object is missing".into()))?;
    let bytes = crate::graph::helpers::load_object_bytes(connection, object_id).await?;
    if bytes != expected {
        return Err(StorageError::Integrity(
            "validated artifact bytes do not match staging replay".into(),
        ));
    }
    let reference = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM content_object_refs WHERE object_id = ? AND owner_kind = 'artifact_staging' AND owner_id = ? AND role = 'validated_content'",
        vec![object_id.into(), staging_id.into()],
    )).await?;
    if reference.is_none() {
        return Err(StorageError::Integrity(
            "validated artifact owner reference is missing".into(),
        ));
    }
    Ok(())
}

pub(super) fn require_uploading(
    owner: &StagingOwner,
    generation: u64,
    now: i64,
) -> StorageResult<()> {
    if owner.status != "uploading" || owner.generation != generation {
        return Err(StorageError::Conflict("artifact_staging_generation"));
    }
    if owner.lease_until.is_none_or(|deadline| deadline <= now) {
        return Err(StorageError::Conflict("artifact_staging_lease_expired"));
    }
    if owner.node_attempt_id.is_some() && owner.node_attempt_status.as_deref() != Some("running") {
        return Err(StorageError::Conflict("artifact_staging_writer_inactive"));
    }
    Ok(())
}

pub(super) async fn quarantine<C: ConnectionTrait>(
    connection: &C,
    command: &CompleteArtifactStagingCommand,
    owner: &StagingOwner,
    now: i64,
) -> StorageResult<()> {
    if owner.status == "quarantined"
        && owner.generation == command.expected_lifecycle_generation.saturating_add(1)
    {
        return Ok(());
    }
    require_uploading(owner, command.expected_lifecycle_generation, now)?;
    let result = connection.execute_raw(sql(
        "UPDATE artifact_staging SET status = 'quarantined', lifecycle_generation = lifecycle_generation + 1, lease_until = NULL, quarantined_at = ?, updated_at = ? WHERE id = ? AND status = 'uploading' AND lifecycle_generation = ?",
        vec![now.into(), now.into(), command.staging_id.clone().into(), command.expected_lifecycle_generation.into()],
    )).await?;
    if result.rows_affected() != 1 {
        return Err(StorageError::Conflict("artifact_staging_generation"));
    }
    Ok(())
}

pub(super) async fn load_view<C: ConnectionTrait>(
    connection: &C,
    id: &str,
) -> StorageResult<ArtifactStagingView> {
    let row = connection
        .query_one_raw(sql(
            &format!("SELECT {VIEW_COLUMNS} FROM artifact_staging WHERE id = ?"),
            vec![id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "artifact_staging",
            id: id.into(),
        })?;
    staging_view(&row)
}

pub(super) async fn add_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    owner_id: &str,
    role: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'artifact_staging', ?, ?, ?)",
        vec![object_id.into(), owner_id.into(), role.into(), now.into()],
    )).await?;
    Ok(())
}
