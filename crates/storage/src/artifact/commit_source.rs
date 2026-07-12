use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::artifact::CommitArtifactStagingCommand,
    artifact::{
        ArtifactClassification, ArtifactMetadata, ArtifactMetadataDraft, ArtifactRef,
        ArtifactRetention, ArtifactStatus,
    },
    canonical,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_bytes, load_object_json, sql},
};

pub(super) struct CommitSource {
    pub context_id: Option<String>,
    pub content_object_id: String,
    pub content_hash: String,
    pub byte_size: u64,
    pub media_type: String,
    pub metadata: ArtifactMetadataDraft,
    pub origin_run_id: Option<String>,
    pub origin_node_instance_id: Option<String>,
    pub origin_tool_call_id: Option<String>,
}

pub(super) async fn load_commit_source<C: ConnectionTrait>(
    connection: &C,
    command: &CommitArtifactStagingCommand,
    now: i64,
) -> StorageResult<CommitSource> {
    let row = connection.query_one_raw(sql(
        "SELECT s.status, s.lifecycle_generation, s.context_id, s.tool_call_id, s.metadata_draft_object_id, s.metadata_draft_digest, s.validated_content_object_id, s.content_hash, s.byte_size, s.validated_media_type, ni.id AS origin_node_instance_id, ni.run_id AS origin_run_id FROM artifact_staging s LEFT JOIN node_attempts a ON a.id = s.node_attempt_id LEFT JOIN node_instances ni ON ni.id = a.node_instance_id WHERE s.id = ?",
        vec![command.staging_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "artifact_staging", id: command.staging_id.clone() })?;
    if row.try_get::<String>("", "status")? != "validated"
        || u64::try_from(row.try_get::<i64>("", "lifecycle_generation")?)
            .map_err(|_| StorageError::Integrity("artifact generation is invalid".into()))?
            != command.expected_lifecycle_generation
    {
        return Err(StorageError::Conflict("artifact_staging_generation"));
    }
    let metadata_object_id: String = row.try_get("", "metadata_draft_object_id")?;
    let metadata_bytes = load_object_bytes(connection, &metadata_object_id).await?;
    if canonical::hash_bytes(&metadata_bytes)
        != row.try_get::<String>("", "metadata_draft_digest")?
    {
        return Err(StorageError::Integrity(
            "artifact metadata draft digest is invalid".into(),
        ));
    }
    let metadata: ArtifactMetadataDraft = load_object_json(connection, &metadata_object_id).await?;
    metadata
        .validate(now)
        .map_err(|message| StorageError::InvalidArgument(message.into()))?;
    let content_object_id: String = row.try_get("", "validated_content_object_id")?;
    let content_hash: String = row.try_get("", "content_hash")?;
    let byte_size = u64::try_from(row.try_get::<i64>("", "byte_size")?)
        .map_err(|_| StorageError::Integrity("artifact byte size is invalid".into()))?;
    let bytes = load_object_bytes(connection, &content_object_id).await?;
    if canonical::hash_bytes(&bytes) != content_hash || bytes.len() as u64 != byte_size {
        return Err(StorageError::Integrity(
            "validated artifact content is corrupt".into(),
        ));
    }
    let reference = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM content_object_refs WHERE object_id = ? AND owner_kind = 'artifact_staging' AND owner_id = ? AND role = 'validated_content'",
        vec![content_object_id.clone().into(), command.staging_id.clone().into()],
    )).await?;
    if reference.is_none() {
        return Err(StorageError::Integrity(
            "validated artifact owner ref is missing".into(),
        ));
    }
    Ok(CommitSource {
        context_id: row.try_get("", "context_id")?,
        content_object_id,
        content_hash,
        byte_size,
        media_type: row.try_get("", "validated_media_type")?,
        metadata,
        origin_run_id: row.try_get("", "origin_run_id")?,
        origin_node_instance_id: row.try_get("", "origin_node_instance_id")?,
        origin_tool_call_id: row.try_get("", "tool_call_id")?,
    })
}

pub(super) fn build_metadata(
    artifact_id: &str,
    source: &CommitSource,
    now: i64,
) -> StorageResult<ArtifactMetadata> {
    let content = ArtifactRef {
        artifact_id: artifact_id.into(),
        content_hash: source.content_hash.clone(),
        byte_size: source.byte_size,
        media_type: source.media_type.clone(),
    };
    content
        .validate()
        .map_err(|message| StorageError::Integrity(message.into()))?;
    Ok(ArtifactMetadata {
        artifact_id: artifact_id.into(),
        content,
        name: source.metadata.name.clone(),
        classification: source.metadata.classification,
        status: ArtifactStatus::Active,
        origin_run_id: source.origin_run_id.clone(),
        origin_node_instance_id: source.origin_node_instance_id.clone(),
        origin_tool_call_id: source.origin_tool_call_id.clone(),
        retention: source.metadata.retention.clone(),
        created_at: now,
    })
}

pub(super) fn classification(value: ArtifactClassification) -> &'static str {
    match value {
        ArtifactClassification::Public => "public",
        ArtifactClassification::Private => "private",
        ArtifactClassification::Sensitive => "sensitive",
    }
}

pub(super) fn retention(value: &ArtifactRetention) -> (&'static str, Option<i64>) {
    match value {
        ArtifactRetention::Ephemeral { expires_at } => ("ephemeral", Some(*expires_at)),
        ArtifactRetention::Run => ("run", None),
        ArtifactRetention::Context => ("context", None),
        ArtifactRetention::Pinned => ("pinned", None),
        ArtifactRetention::AuditUntil { timestamp } => ("audit_until", Some(*timestamp)),
    }
}
