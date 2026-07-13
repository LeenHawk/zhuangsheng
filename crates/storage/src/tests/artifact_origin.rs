use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::artifact::{
        CommitArtifactStagingCommand, CompleteArtifactStagingCommand, CreateArtifactStagingCommand,
    },
    artifact::{ArtifactClassification, ArtifactMetadataDraft, ArtifactRetention},
};

use crate::{
    graph::helpers::sql,
    tests::{
        llm_ledger::{now_ms, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn node_owned_staging_pins_run_context_and_commit_origin() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let now = now_ms();
    let staging = store
        .create_artifact_staging_at(
            CreateArtifactStagingCommand {
                context_id: None,
                node_attempt_id: Some(claimed.attempt_id.clone()),
                tool_call_id: None,
                metadata_draft: ArtifactMetadataDraft {
                    name: Some("node-note.txt".into()),
                    classification: ArtifactClassification::Private,
                    retention: ArtifactRetention::Run,
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
                bytes: b"node-owned artifact".to_vec(),
            },
            now + 1,
        )
        .await
        .unwrap();
    let artifact = store
        .commit_artifact_staging_at(
            CommitArtifactStagingCommand {
                staging_id: staging.staging_id,
                expected_lifecycle_generation: 2,
                idempotency_key: "commit-node-artifact".into(),
            },
            now + 2,
        )
        .await
        .unwrap();
    assert_eq!(
        artifact.metadata.origin_run_id,
        Some(claimed.run_id.clone())
    );
    assert_eq!(
        artifact.metadata.origin_node_instance_id,
        Some(claimed.node_instance_id)
    );
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT a.context_id, r.context_id AS run_context_id FROM artifacts a JOIN graph_runs r ON r.id = a.origin_run_id WHERE a.id = ?",
            vec![artifact.metadata.artifact_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "context_id").unwrap(),
        row.try_get::<String>("", "run_context_id").unwrap()
    );
    let event = store
        .db
        .query_one_raw(sql(
            "SELECT payload_json FROM run_events WHERE run_id = ? AND event_type = 'artifact.committed'",
            vec![claimed.run_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(&event.try_get::<String>("", "payload_json").unwrap()).unwrap();
    assert_eq!(
        payload
            .pointer("/artifactId")
            .and_then(serde_json::Value::as_str),
        Some(artifact.metadata.artifact_id.as_str())
    );
}
