use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::artifact::CommitArtifactStagingCommand, artifact::ArtifactView, canonical,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
    runtime::{Event, append_event},
};

use super::{
    commit_source::{build_metadata, classification, load_commit_source, retention},
    read::load_artifact_view,
};

impl SqliteStore {
    pub async fn commit_artifact_staging_at(
        &self,
        command: CommitArtifactStagingCommand,
        now: i64,
    ) -> StorageResult<ArtifactView> {
        validate_command(&command)?;
        let digest = canonical::hash(&json!({
            "schemaVersion":1,
            "stagingId":command.staging_id,
            "expectedLifecycleGeneration":command.expected_lifecycle_generation,
        }))?;
        let scope = format!("artifact-staging:{}", command.staging_id);
        let transaction = self.db.begin().await?;
        if let Some(replayed) =
            replay(&transaction, &scope, &command.idempotency_key, &digest).await?
        {
            let view = load_artifact_view(&transaction, &replayed.metadata.artifact_id).await?;
            if view != replayed {
                return Err(StorageError::Integrity(
                    "artifact commit receipt is corrupt".into(),
                ));
            }
            transaction.commit().await?;
            return Ok(view);
        }
        let source = load_commit_source(&transaction, &command, now).await?;
        let artifact_id = new_id("artifact");
        let commit_id = new_id("commit");
        let metadata = build_metadata(&artifact_id, &source, now)?;
        let snapshot_id =
            put_inline_object(&transaction, &canonical::to_vec(&metadata)?, now).await?;
        let actor_kind = if source.origin_tool_call_id.is_some() {
            "tool"
        } else {
            "application"
        };
        transaction.execute_raw(sql(
            "INSERT INTO version_commits (id, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, initial_snapshot_object_id, schema_version, policy_version, author_kind, author_id, origin_run_id, origin_node_instance_id, created_at) VALUES (?, 'artifact_metadata', ?, 'global', 1, ?, ?, 1, 1, ?, ?, ?, ?, ?)",
            vec![commit_id.clone().into(), artifact_id.clone().into(), format!("artifact-commit:{}", command.idempotency_key).into(), snapshot_id.clone().into(), actor_kind.into(), source.origin_tool_call_id.clone().into(), source.origin_run_id.clone().into(), source.origin_node_instance_id.clone().into(), now.into()],
        )).await?;
        let (retention_kind, retention_until) = retention(&metadata.retention);
        transaction.execute_raw(sql(
            "INSERT INTO artifacts (id, context_id, source_staging_id, content_object_id, metadata_head_commit_id, media_type, name, classification, retention_kind, retention_until, status, origin_run_id, origin_node_instance_id, origin_tool_call_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'active', ?, ?, ?, ?, ?)",
            vec![artifact_id.clone().into(), source.context_id.into(), command.staging_id.clone().into(), source.content_object_id.clone().into(), commit_id.clone().into(), source.media_type.into(), metadata.name.clone().into(), classification(metadata.classification).into(), retention_kind.into(), retention_until.into(), source.origin_run_id.into(), source.origin_node_instance_id.into(), source.origin_tool_call_id.into(), now.into(), now.into()],
        )).await?;
        transaction.execute_raw(sql(
            "INSERT INTO materialized_projections (aggregate_kind, aggregate_id, lineage_key, head_commit_id, projection_json, schema_version, updated_at) VALUES ('artifact_metadata', ?, 'global', ?, ?, 1, ?)",
            vec![artifact_id.clone().into(), commit_id.clone().into(), canonical::to_string(&metadata)?.into(), now.into()],
        )).await?;
        add_ref(
            &transaction,
            &snapshot_id,
            "version_commit",
            &commit_id,
            "initial_snapshot",
            now,
        )
        .await?;
        add_ref(
            &transaction,
            &source.content_object_id,
            "artifact",
            &artifact_id,
            "content",
            now,
        )
        .await?;
        let view = ArtifactView {
            metadata,
            metadata_head_commit_id: commit_id,
        };
        let result_id = put_inline_object(&transaction, &canonical::to_vec(&view)?, now).await?;
        let updated = transaction.execute_raw(sql(
            "UPDATE artifact_staging SET status = 'committed', lifecycle_generation = lifecycle_generation + 1, commit_request_digest = ?, committed_artifact_id = ?, commit_result_object_id = ?, committed_at = ?, updated_at = ? WHERE id = ? AND status = 'validated' AND lifecycle_generation = ?",
            vec![digest.clone().into(), artifact_id.clone().into(), result_id.clone().into(), now.into(), now.into(), command.staging_id.clone().into(), command.expected_lifecycle_generation.into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("artifact_staging_generation"));
        }
        transaction.execute_raw(sql(
            "DELETE FROM content_object_refs WHERE object_id = ? AND owner_kind = 'artifact_staging' AND owner_id = ? AND role = 'validated_content'",
            vec![source.content_object_id.into(), command.staging_id.clone().into()],
        )).await?;
        add_ref(
            &transaction,
            &result_id,
            "artifact_staging",
            &command.staging_id,
            "commit_result",
            now,
        )
        .await?;
        finish_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &artifact_id,
            &result_id,
            now,
        )
        .await?;
        if let Some(run_id) = &view.metadata.origin_run_id {
            append_event(
                &transaction,
                Event {
                    run_id,
                    event_type: "artifact.committed",
                    importance: "critical",
                    node_instance_id: view.metadata.origin_node_instance_id.as_deref(),
                    attempt_id: None,
                    payload: json!({
                        "schemaVersion":1,
                        "artifactId":view.metadata.artifact_id,
                        "artifactRef":view.metadata.content,
                        "metadataCommitId":view.metadata_head_commit_id,
                        "originToolCallId":view.metadata.origin_tool_call_id,
                    }),
                    now,
                },
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(view)
    }
}

fn validate_command(command: &CommitArtifactStagingCommand) -> StorageResult<()> {
    if command.staging_id.is_empty()
        || command.staging_id.len() > 128
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid artifact commit command".into(),
        ));
    }
    Ok(())
}

async fn replay<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<ArtifactView>> {
    let row = connection.query_one_raw(sql("SELECT request_digest, result_object_id FROM application_command_receipts WHERE scope = ? AND idempotency_key = ? AND status = 'completed'", vec![scope.into(), key.into()])).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    load_object_json(connection, &row.try_get::<String>("", "result_object_id")?)
        .await
        .map(Some)
}

async fn finish_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
    artifact_id: &str,
    result_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES (?, ?, ?, 'artifact.commit', 'artifact', ?, 'completed', ?, ?, ?)",
        vec![scope.into(), key.into(), digest.into(), artifact_id.into(), result_id.into(), now.into(), now.into()],
    )).await?;
    add_ref(
        connection,
        result_id,
        "application_receipt",
        &format!("{scope}:{key}"),
        "result",
        now,
    )
    .await
}

async fn add_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    owner_kind: &str,
    owner_id: &str,
    role: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql("INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, ?, ?, ?, ?)", vec![object_id.into(), owner_kind.into(), owner_id.into(), role.into(), now.into()])).await?;
    Ok(())
}
