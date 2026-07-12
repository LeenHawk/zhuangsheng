use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    artifact::{ArtifactMetadata, ArtifactStatus, ArtifactView},
    canonical,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_bytes, load_object_json, sql},
};

use super::commit_source::{classification, retention};

pub(super) async fn load_artifact_view<C: ConnectionTrait>(
    connection: &C,
    artifact_id: &str,
) -> StorageResult<ArtifactView> {
    let row = connection.query_one_raw(sql(
        "SELECT a.content_object_id, a.metadata_head_commit_id, a.media_type, a.name, a.classification, a.retention_kind, a.retention_until, a.status, a.origin_run_id, a.origin_node_instance_id, a.origin_tool_call_id, a.created_at, p.projection_json, p.head_commit_id AS projection_head, vc.initial_snapshot_object_id, vc.sequence_no, s.status AS staging_status, s.committed_artifact_id, s.validated_content_object_id FROM artifacts a JOIN materialized_projections p ON p.aggregate_kind = 'artifact_metadata' AND p.aggregate_id = a.id AND p.lineage_key = 'global' JOIN version_commits vc ON vc.id = a.metadata_head_commit_id AND vc.aggregate_kind = 'artifact_metadata' AND vc.aggregate_id = a.id AND vc.lineage_key = 'global' JOIN artifact_staging s ON s.id = a.source_staging_id WHERE a.id = ?",
        vec![artifact_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "artifact", id: artifact_id.into() })?;
    let metadata: ArtifactMetadata =
        serde_json::from_str(&row.try_get::<String>("", "projection_json")?)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let head: String = row.try_get("", "metadata_head_commit_id")?;
    let (retention_kind, retention_until) = retention(&metadata.retention);
    if metadata.artifact_id != artifact_id
        || metadata.content.media_type != row.try_get::<String>("", "media_type")?
        || metadata.name != row.try_get::<Option<String>>("", "name")?
        || classification(metadata.classification) != row.try_get::<String>("", "classification")?
        || retention_kind != row.try_get::<String>("", "retention_kind")?
        || retention_until != row.try_get::<Option<i64>>("", "retention_until")?
        || metadata.origin_run_id != row.try_get::<Option<String>>("", "origin_run_id")?
        || metadata.origin_node_instance_id
            != row.try_get::<Option<String>>("", "origin_node_instance_id")?
        || metadata.origin_tool_call_id
            != row.try_get::<Option<String>>("", "origin_tool_call_id")?
        || metadata.created_at != row.try_get::<i64>("", "created_at")?
        || head != row.try_get::<String>("", "projection_head")?
        || row.try_get::<i64>("", "sequence_no")? != 1
        || row.try_get::<String>("", "staging_status")? != "committed"
        || row.try_get::<String>("", "committed_artifact_id")? != artifact_id
        || metadata.status != ArtifactStatus::Active
        || row.try_get::<String>("", "status")? != "active"
    {
        return Err(StorageError::Integrity(
            "artifact projection is corrupt".into(),
        ));
    }
    let snapshot: ArtifactMetadata = load_object_json(
        connection,
        &row.try_get::<String>("", "initial_snapshot_object_id")?,
    )
    .await?;
    if snapshot != metadata {
        return Err(StorageError::Integrity(
            "artifact metadata snapshot is corrupt".into(),
        ));
    }
    metadata
        .content
        .validate()
        .map_err(|message| StorageError::Integrity(message.into()))?;
    let object_id: String = row.try_get("", "content_object_id")?;
    if row.try_get::<String>("", "validated_content_object_id")? != object_id {
        return Err(StorageError::Integrity(
            "artifact staging content binding is corrupt".into(),
        ));
    }
    let bytes = load_object_bytes(connection, &object_id).await?;
    if canonical::hash_bytes(&bytes) != metadata.content.content_hash
        || bytes.len() as u64 != metadata.content.byte_size
    {
        return Err(StorageError::Integrity(
            "artifact content is corrupt".into(),
        ));
    }
    let reference = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM content_object_refs WHERE object_id = ? AND owner_kind = 'artifact' AND owner_id = ? AND role = 'content'",
        vec![object_id.into(), artifact_id.into()],
    )).await?;
    if reference.is_none() {
        return Err(StorageError::Integrity(
            "artifact content owner ref is missing".into(),
        ));
    }
    Ok(ArtifactView {
        metadata,
        metadata_head_commit_id: head,
    })
}

impl SqliteStore {
    pub async fn get_artifact_view(&self, artifact_id: &str) -> StorageResult<ArtifactView> {
        if artifact_id.is_empty() || artifact_id.len() > 128 {
            return Err(StorageError::InvalidArgument(
                "artifact id is invalid".into(),
            ));
        }
        load_artifact_view(&self.db, artifact_id).await
    }
}
