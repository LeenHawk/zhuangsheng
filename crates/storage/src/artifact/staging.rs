use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    application::artifact::{CompleteArtifactStagingCommand, CreateArtifactStagingCommand},
    artifact::{ArtifactRetention, ArtifactStagingView},
    canonical,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{
    staging_owner::validate_owner,
    staging_support::{
        add_ref, load_owner, load_view, quarantine, require_uploading, validate_replay_content,
    },
    validation::{validate_bytes, validate_create},
};

const WRITER_LEASE_MS: i64 = 5 * 60 * 1000;
const STAGING_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

impl SqliteStore {
    pub async fn create_artifact_staging_at(
        &self,
        command: CreateArtifactStagingCommand,
        now: i64,
    ) -> StorageResult<ArtifactStagingView> {
        validate_create(&command, now)?;
        let metadata = canonical::to_vec(&command.metadata_draft)?;
        let metadata_digest = canonical::hash_bytes(&metadata);
        let expires_at = match command.metadata_draft.retention {
            ArtifactRetention::Ephemeral { expires_at } => {
                expires_at.min(now.saturating_add(STAGING_RETENTION_MS))
            }
            _ => now.saturating_add(STAGING_RETENTION_MS),
        };
        let staging_id = new_id("staging");
        let transaction = self.db.begin().await?;
        let context_id = validate_owner(&transaction, &command).await?;
        let metadata_object_id = put_inline_object(&transaction, &metadata, now).await?;
        transaction.execute_raw(sql(
            "INSERT INTO artifact_staging (id, context_id, node_attempt_id, tool_call_id, expected_media_type, metadata_draft_object_id, metadata_draft_digest, status, lifecycle_generation, lease_until, expires_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'uploading', 0, ?, ?, ?, ?)",
            vec![staging_id.clone().into(), context_id.into(), command.node_attempt_id.into(), command.tool_call_id.into(), command.declared_media_type.into(), metadata_object_id.clone().into(), metadata_digest.into(), now.saturating_add(WRITER_LEASE_MS).min(expires_at).into(), expires_at.into(), now.into(), now.into()],
        )).await?;
        add_ref(
            &transaction,
            &metadata_object_id,
            &staging_id,
            "metadata_draft",
            now,
        )
        .await?;
        let view = load_view(&transaction, &staging_id).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn complete_artifact_staging_at(
        &self,
        command: CompleteArtifactStagingCommand,
        now: i64,
    ) -> StorageResult<ArtifactStagingView> {
        if command.staging_id.is_empty() || command.staging_id.len() > 128 {
            return Err(StorageError::InvalidArgument(
                "artifact staging id is invalid".into(),
            ));
        }
        let content_hash = canonical::hash_bytes(&command.bytes);
        let transaction = self.db.begin().await?;
        let owner = load_owner(&transaction, &command.staging_id).await?;
        let detected = validate_bytes(&command.bytes, owner.declared_media_type.as_deref());
        if detected.is_err() {
            quarantine(&transaction, &command, &owner, now).await?;
            let view = load_view(&transaction, &command.staging_id).await?;
            transaction.commit().await?;
            return Ok(view);
        }
        let media_type = detected.expect("validated above");
        if owner.status == "validated"
            && owner.generation == command.expected_lifecycle_generation.saturating_add(2)
            && owner.content_hash.as_deref() == Some(&content_hash)
            && owner.byte_size == Some(command.bytes.len() as i64)
            && owner.validated_media_type.as_deref() == Some(media_type)
        {
            validate_replay_content(&transaction, &owner, &command.staging_id, &command.bytes)
                .await?;
            let view = load_view(&transaction, &command.staging_id).await?;
            transaction.commit().await?;
            return Ok(view);
        }
        require_uploading(&owner, command.expected_lifecycle_generation, now)?;
        let staged = transaction.execute_raw(sql(
            "UPDATE artifact_staging SET status = 'staged', lifecycle_generation = lifecycle_generation + 1, byte_size = ?, content_hash = ?, lease_until = NULL, updated_at = ? WHERE id = ? AND status = 'uploading' AND lifecycle_generation = ?",
            vec![(command.bytes.len() as i64).into(), content_hash.clone().into(), now.into(), command.staging_id.clone().into(), command.expected_lifecycle_generation.into()],
        )).await?;
        if staged.rows_affected() != 1 {
            return Err(StorageError::Conflict("artifact_staging_generation"));
        }
        let object_id = put_inline_object(&transaction, &command.bytes, now).await?;
        add_ref(
            &transaction,
            &object_id,
            &command.staging_id,
            "validated_content",
            now,
        )
        .await?;
        let validated = transaction.execute_raw(sql(
            "UPDATE artifact_staging SET status = 'validated', lifecycle_generation = lifecycle_generation + 1, validated_media_type = ?, validated_content_object_id = ?, updated_at = ? WHERE id = ? AND status = 'staged' AND lifecycle_generation = ?",
            vec![media_type.into(), object_id.into(), now.into(), command.staging_id.clone().into(), command.expected_lifecycle_generation.saturating_add(1).into()],
        )).await?;
        if validated.rows_affected() != 1 {
            return Err(StorageError::Conflict("artifact_staging_generation"));
        }
        let view = load_view(&transaction, &command.staging_id).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn get_artifact_staging_view(
        &self,
        staging_id: &str,
    ) -> StorageResult<ArtifactStagingView> {
        load_view(&self.db, staging_id).await
    }
}
