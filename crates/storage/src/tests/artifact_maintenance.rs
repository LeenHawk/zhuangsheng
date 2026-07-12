use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::artifact::{CompleteArtifactStagingCommand, CreateArtifactStagingCommand},
    artifact::{
        ArtifactClassification, ArtifactMetadataDraft, ArtifactRetention, ArtifactStagingStatus,
    },
};

use crate::{graph::helpers::sql, tests::store};

const NOW: i64 = 1_700_000_000_000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const GRACE_MS: i64 = 60_000;

#[tokio::test]
async fn staging_maintenance_quarantines_expires_and_fenced_deletes() {
    let store = store().await;
    let uploading = store
        .create_artifact_staging_at(create("unfinished.txt"), NOW)
        .await
        .unwrap();
    let validated = store
        .create_artifact_staging_at(create("validated.txt"), NOW)
        .await
        .unwrap();
    store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: validated.staging_id.clone(),
                expected_lifecycle_generation: 0,
                bytes: b"uncommitted artifact".to_vec(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    assert_eq!(validated_refs(&store).await, 1);

    let report = store
        .maintain_artifact_staging(NOW + DAY_MS, GRACE_MS, 100)
        .await
        .unwrap();
    assert_eq!(report.quarantined, 2);
    assert_eq!(report.deleting, 0);
    assert_eq!(report.deleted, 0);
    assert_eq!(
        store
            .get_artifact_staging_view(&uploading.staging_id)
            .await
            .unwrap()
            .status,
        ArtifactStagingStatus::Quarantined
    );
    assert_eq!(validated_refs(&store).await, 1);

    let report = store
        .maintain_artifact_staging(NOW + DAY_MS + GRACE_MS, GRACE_MS, 100)
        .await
        .unwrap();
    assert_eq!(report.quarantined, 0);
    assert_eq!(report.deleting, 2);
    assert_eq!(report.deleted, 2);
    let deleted = store
        .get_artifact_staging_view(&validated.staging_id)
        .await
        .unwrap();
    assert_eq!(deleted.status, ArtifactStagingStatus::Deleted);
    assert_eq!(deleted.lifecycle_generation, 5);
    assert_eq!(validated_refs(&store).await, 0);
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT validated_content_object_id, delete_fence, deleted_at FROM artifact_staging WHERE id = ?",
            vec![validated.staging_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        row.try_get::<Option<String>>("", "validated_content_object_id")
            .unwrap()
            .is_none()
    );
    assert!(
        !row.try_get::<String>("", "delete_fence")
            .unwrap()
            .is_empty()
    );
    assert!(row.try_get::<i64>("", "deleted_at").unwrap() > NOW);
}

fn create(name: &str) -> CreateArtifactStagingCommand {
    CreateArtifactStagingCommand {
        context_id: None,
        node_attempt_id: None,
        tool_call_id: None,
        metadata_draft: ArtifactMetadataDraft {
            name: Some(name.into()),
            classification: ArtifactClassification::Private,
            retention: ArtifactRetention::Pinned,
        },
        declared_media_type: Some("text/plain".into()),
    }
}

async fn validated_refs(store: &crate::SqliteStore) -> i64 {
    store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM content_object_refs WHERE owner_kind = 'artifact_staging' AND role = 'validated_content'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
