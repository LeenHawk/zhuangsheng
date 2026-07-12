use zhuangsheng_core::{
    application::artifact::{
        CommitArtifactStagingCommand, CompleteArtifactStagingCommand, CreateArtifactStagingCommand,
    },
    artifact::{ArtifactClassification, ArtifactMetadataDraft, ArtifactRetention},
};

use crate::tests::store;

#[tokio::test]
async fn artifact_list_is_bounded_newest_first_and_metadata_only() {
    let store = store().await;
    let first = create(&store, "first.txt", 1_700_002_000_000).await;
    let second = create(&store, "second.txt", 1_700_002_000_010).await;
    let page = store.list_artifact_views(1).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].metadata.artifact_id, second);
    assert_ne!(page.items[0].metadata.artifact_id, first);
    assert!(store.list_artifact_views(0).await.is_err());
    assert!(store.list_artifact_views(101).await.is_err());
}

async fn create(store: &crate::SqliteStore, name: &str, now: i64) -> String {
    let staging = store
        .create_artifact_staging_at(
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
            },
            now,
        )
        .await
        .unwrap();
    let staged = store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: staging.staging_id,
                expected_lifecycle_generation: 0,
                bytes: name.as_bytes().to_vec(),
            },
            now + 1,
        )
        .await
        .unwrap();
    store
        .commit_artifact_staging_at(
            CommitArtifactStagingCommand {
                staging_id: staged.staging_id,
                expected_lifecycle_generation: staged.lifecycle_generation,
                idempotency_key: format!("commit-{name}"),
            },
            now + 2,
        )
        .await
        .unwrap()
        .metadata
        .artifact_id
}
