use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::artifact::{CompleteArtifactStagingCommand, CreateArtifactStagingCommand},
    artifact::{
        ArtifactClassification, ArtifactMetadataDraft, ArtifactRetention, ArtifactStagingStatus,
    },
};

use crate::{StorageError, graph::helpers::sql, tests::store};

const NOW: i64 = 1_700_000_000_000;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nminimal-test-payload";

#[tokio::test]
async fn staging_validates_deduplicates_and_replays_without_exposing_object_ids() {
    let store = store().await;
    let uploading = store
        .create_artifact_staging_at(create("portrait.png", Some("image/png")), NOW)
        .await
        .unwrap();
    assert_eq!(uploading.status, ArtifactStagingStatus::Uploading);
    assert_eq!(uploading.lifecycle_generation, 0);
    assert_eq!(uploading.content_hash, None);

    let command = CompleteArtifactStagingCommand {
        staging_id: uploading.staging_id.clone(),
        expected_lifecycle_generation: 0,
        bytes: PNG.to_vec(),
    };
    let validated = store
        .complete_artifact_staging_at(command.clone(), NOW + 1)
        .await
        .unwrap();
    assert_eq!(validated.status, ArtifactStagingStatus::Validated);
    assert_eq!(validated.lifecycle_generation, 2);
    assert_eq!(validated.byte_size, Some(PNG.len() as u64));
    assert_eq!(validated.validated_media_type.as_deref(), Some("image/png"));
    assert_eq!(
        store
            .complete_artifact_staging_at(command, NOW + 2)
            .await
            .unwrap(),
        validated
    );
    assert_eq!(
        store
            .get_artifact_staging_view(&uploading.staging_id)
            .await
            .unwrap(),
        validated
    );

    let second = store
        .create_artifact_staging_at(
            create("copy.png", Some("application/octet-stream")),
            NOW + 3,
        )
        .await
        .unwrap();
    store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: second.staging_id,
                expected_lifecycle_generation: 0,
                bytes: PNG.to_vec(),
            },
            NOW + 4,
        )
        .await
        .unwrap();
    assert_eq!(count(&store, "content", None).await, 3);
    assert_eq!(count(&store, "validated_refs", None).await, 2);

    let conflict = store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: uploading.staging_id,
                expected_lifecycle_generation: 0,
                bytes: b"different text".to_vec(),
            },
            NOW + 5,
        )
        .await;
    assert!(matches!(
        conflict,
        Err(StorageError::Conflict("artifact_staging_generation"))
    ));
}

#[tokio::test]
async fn malformed_upload_is_quarantined_and_invalid_metadata_writes_nothing() {
    let store = store().await;
    let invalid = store
        .create_artifact_staging_at(create("payload.bin", None), NOW)
        .await
        .unwrap();
    let quarantined = store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: invalid.staging_id.clone(),
                expected_lifecycle_generation: 0,
                bytes: vec![0, 159, 146, 150],
            },
            NOW + 1,
        )
        .await
        .unwrap();
    assert_eq!(quarantined.status, ArtifactStagingStatus::Quarantined);
    assert_eq!(quarantined.lifecycle_generation, 1);
    assert_eq!(quarantined.content_hash, None);
    assert_eq!(count(&store, "validated_refs", None).await, 0);

    let mut command = create("../escape.txt", Some("text/plain"));
    command.metadata_draft.retention = ArtifactRetention::Ephemeral {
        expires_at: NOW - 1,
    };
    assert!(matches!(
        store.create_artifact_staging_at(command, NOW).await,
        Err(StorageError::InvalidArgument(_))
    ));
    assert_eq!(count(&store, "staging", None).await, 1);
}

#[tokio::test]
async fn staging_fails_closed_on_metadata_or_deduplicated_object_corruption() {
    let store = store().await;
    let first = store
        .create_artifact_staging_at(create("first.png", Some("image/png")), NOW)
        .await
        .unwrap();
    let first = store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: first.staging_id,
                expected_lifecycle_generation: 0,
                bytes: PNG.to_vec(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    store
        .db
        .execute_raw(sql(
            "UPDATE content_objects SET inline_bytes = X'00' WHERE content_hash = ?",
            vec![first.content_hash.unwrap().into()],
        ))
        .await
        .unwrap();
    let second = store
        .create_artifact_staging_at(create("second.png", Some("image/png")), NOW + 2)
        .await
        .unwrap();
    assert!(matches!(
        store
            .complete_artifact_staging_at(
                CompleteArtifactStagingCommand {
                    staging_id: second.staging_id,
                    expected_lifecycle_generation: 0,
                    bytes: PNG.to_vec(),
                },
                NOW + 3,
            )
            .await,
        Err(StorageError::Integrity(_))
    ));

    let metadata = store
        .create_artifact_staging_at(create("metadata.png", Some("image/png")), NOW + 4)
        .await
        .unwrap();
    store
        .db
        .execute_raw(sql(
            "UPDATE artifact_staging SET metadata_draft_digest = 'sha256:tampered' WHERE id = ?",
            vec![metadata.staging_id.clone().into()],
        ))
        .await
        .unwrap();
    assert!(matches!(
        store
            .complete_artifact_staging_at(
                CompleteArtifactStagingCommand {
                    staging_id: metadata.staging_id,
                    expected_lifecycle_generation: 0,
                    bytes: PNG.to_vec(),
                },
                NOW + 5,
            )
            .await,
        Err(StorageError::Integrity(_))
    ));
}

fn create(name: &str, declared_media_type: Option<&str>) -> CreateArtifactStagingCommand {
    CreateArtifactStagingCommand {
        context_id: None,
        node_attempt_id: None,
        tool_call_id: None,
        metadata_draft: ArtifactMetadataDraft {
            name: Some(name.into()),
            classification: ArtifactClassification::Private,
            retention: ArtifactRetention::Pinned,
        },
        declared_media_type: declared_media_type.map(str::to_owned),
    }
}

async fn count(store: &crate::SqliteStore, kind: &str, _id: Option<&str>) -> i64 {
    let query = match kind {
        "content" => "SELECT COUNT(*) AS count FROM content_objects",
        "validated_refs" => {
            "SELECT COUNT(*) AS count FROM content_object_refs WHERE owner_kind = 'artifact_staging' AND role = 'validated_content'"
        }
        "staging" => "SELECT COUNT(*) AS count FROM artifact_staging",
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
