use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::artifact::{
        CommitArtifactStagingCommand, CompleteArtifactStagingCommand, CreateArtifactStagingCommand,
    },
    artifact::{
        ArtifactClassification, ArtifactMetadataDraft, ArtifactRetention, ArtifactStagingStatus,
        ArtifactStatus,
    },
};

use crate::{StorageError, graph::helpers::sql, tests::store};

const NOW: i64 = 1_700_000_000_000;
const BODY: &[u8] = b"artifact commit payload";

#[tokio::test]
async fn validated_staging_commits_metadata_content_and_receipt_atomically() {
    let store = store().await;
    let staging_id = validated(&store, "story.txt", ArtifactRetention::Pinned, NOW).await;
    let command = commit(&staging_id, 2, "commit-story");
    let artifact = store
        .commit_artifact_staging_at(command.clone(), NOW + 2)
        .await
        .unwrap();
    assert_eq!(artifact.metadata.status, ArtifactStatus::Active);
    assert_eq!(artifact.metadata.name.as_deref(), Some("story.txt"));
    assert_eq!(artifact.metadata.content.media_type, "text/plain");
    assert_eq!(artifact.metadata.content.byte_size, BODY.len() as u64);
    assert_eq!(
        store
            .get_artifact_staging_view(&staging_id)
            .await
            .unwrap()
            .status,
        ArtifactStagingStatus::Committed
    );
    assert_eq!(
        store
            .get_artifact_view(&artifact.metadata.artifact_id)
            .await
            .unwrap(),
        artifact
    );
    assert_eq!(
        store
            .commit_artifact_staging_at(command, NOW + 3)
            .await
            .unwrap(),
        artifact
    );
    assert_eq!(count(&store, "artifact_content_ref").await, 1);
    assert_eq!(count(&store, "staging_content_ref").await, 0);
    assert_eq!(count(&store, "artifact_commit").await, 1);
    assert_eq!(count(&store, "artifact_projection").await, 1);

    let mismatch = store
        .commit_artifact_staging_at(commit(&staging_id, 99, "commit-story"), NOW + 4)
        .await;
    assert!(matches!(mismatch, Err(StorageError::IdempotencyConflict)));
    let another_key = store
        .commit_artifact_staging_at(commit(&staging_id, 2, "another-key"), NOW + 5)
        .await;
    assert!(matches!(
        another_key,
        Err(StorageError::Conflict("artifact_staging_generation"))
    ));
}

#[tokio::test]
async fn commit_revalidates_retention_and_projection_integrity() {
    let store = store().await;
    let staging_id = validated(
        &store,
        "short.txt",
        ArtifactRetention::Ephemeral {
            expires_at: NOW + 5,
        },
        NOW,
    )
    .await;
    assert!(matches!(
        store
            .commit_artifact_staging_at(commit(&staging_id, 2, "expired"), NOW + 6)
            .await,
        Err(StorageError::InvalidArgument(_))
    ));
    assert_eq!(count(&store, "artifact").await, 0);
    assert_eq!(
        store
            .get_artifact_staging_view(&staging_id)
            .await
            .unwrap()
            .status,
        ArtifactStagingStatus::Validated
    );

    let valid_id = validated(&store, "valid.txt", ArtifactRetention::Pinned, NOW + 10).await;
    let artifact = store
        .commit_artifact_staging_at(commit(&valid_id, 2, "valid"), NOW + 12)
        .await
        .unwrap();
    store
        .db
        .execute_raw(sql(
            "UPDATE materialized_projections SET projection_json = '{}' WHERE aggregate_kind = 'artifact_metadata' AND aggregate_id = ?",
            vec![artifact.metadata.artifact_id.clone().into()],
        ))
        .await
        .unwrap();
    assert!(matches!(
        store
            .get_artifact_view(&artifact.metadata.artifact_id)
            .await,
        Err(StorageError::Integrity(_))
    ));
}

async fn validated(
    store: &crate::SqliteStore,
    name: &str,
    retention: ArtifactRetention,
    now: i64,
) -> String {
    let staging = store
        .create_artifact_staging_at(
            CreateArtifactStagingCommand {
                context_id: None,
                node_attempt_id: None,
                tool_call_id: None,
                metadata_draft: ArtifactMetadataDraft {
                    name: Some(name.into()),
                    classification: ArtifactClassification::Private,
                    retention,
                },
                declared_media_type: Some("text/plain".into()),
            },
            now,
        )
        .await
        .unwrap();
    store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: staging.staging_id.clone(),
                expected_lifecycle_generation: 0,
                bytes: BODY.to_vec(),
            },
            now + 1,
        )
        .await
        .unwrap();
    staging.staging_id
}

fn commit(staging_id: &str, generation: u64, key: &str) -> CommitArtifactStagingCommand {
    CommitArtifactStagingCommand {
        staging_id: staging_id.into(),
        expected_lifecycle_generation: generation,
        idempotency_key: key.into(),
    }
}

async fn count(store: &crate::SqliteStore, kind: &str) -> i64 {
    let query = match kind {
        "artifact" => "SELECT COUNT(*) AS count FROM artifacts",
        "artifact_content_ref" => {
            "SELECT COUNT(*) AS count FROM content_object_refs WHERE owner_kind = 'artifact' AND role = 'content'"
        }
        "staging_content_ref" => {
            "SELECT COUNT(*) AS count FROM content_object_refs WHERE owner_kind = 'artifact_staging' AND role = 'validated_content'"
        }
        "artifact_commit" => {
            "SELECT COUNT(*) AS count FROM version_commits WHERE aggregate_kind = 'artifact_metadata'"
        }
        "artifact_projection" => {
            "SELECT COUNT(*) AS count FROM materialized_projections WHERE aggregate_kind = 'artifact_metadata'"
        }
        _ => unreachable!(),
    };
    store
        .db
        .query_one_raw(sql(query, vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
