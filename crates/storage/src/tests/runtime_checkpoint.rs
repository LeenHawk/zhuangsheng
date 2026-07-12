use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::Scheduler,
};

use crate::{
    StorageError,
    graph::helpers::{now_ms, sql},
};

use super::{applied_graph, store};

#[tokio::test]
async fn checkpoint_captures_a_consistent_slice_and_replays_its_journal_tail() {
    let store = store().await;
    let revision = applied_graph(&store, "checkpoint").await;
    let run = store
        .start_run(start(&revision.id, "checkpoint-run"))
        .await
        .unwrap();

    let checkpoint = store.create_runtime_checkpoint(&run.id, 100).await.unwrap();
    assert_eq!(checkpoint.through_seq, run.last_durable_seq);
    assert_eq!(checkpoint.head_commit_id, run.input_commit_id);
    let recovered = store.recover_runtime_runs().await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].checkpoint_id, checkpoint.id);
    assert_eq!(recovered[0].replayed_event_count, 1);
    assert!(recovered[0].projection_consistent);
    store
        .maintain_content_objects(70_101, 60_000, 1_000)
        .await
        .unwrap();
    let lifecycle: String = store
        .db
        .query_one_raw(sql(
            "SELECT lifecycle FROM content_objects WHERE id=?",
            vec![checkpoint.snapshot_ref.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "lifecycle")
        .unwrap();
    assert_eq!(lifecycle, "live");
}

#[tokio::test]
async fn restart_creates_missing_checkpoint_and_projection_drift_fails_closed() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", file.path().display());
    let run_id = {
        let store = crate::SqliteStore::connect(&url).await.unwrap();
        let revision = applied_graph(&store, "checkpoint-restart").await;
        store
            .start_run(start(&revision.id, "checkpoint-restart-run"))
            .await
            .unwrap()
            .id
    };
    let reopened = crate::SqliteStore::connect(&url).await.unwrap();
    let recovered = reopened.recover_runtime_runs().await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].run_id, run_id);

    reopened
        .db
        .execute_raw(sql(
            "UPDATE run_execution_counters SET pending_queue_values=1 WHERE run_id=?",
            vec![run_id.into()],
        ))
        .await
        .unwrap();
    assert!(matches!(
        reopened.recover_runtime_runs().await,
        Err(StorageError::Integrity(_))
    ));
}

#[tokio::test]
async fn scheduler_checkpoints_each_committed_work_boundary() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "checkpoint-scheduler").await;
    let run = store
        .start_run(start(&revision.id, "checkpoint-scheduler-run"))
        .await
        .unwrap();

    Scheduler::new(store.clone(), "checkpoint-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    let count: i64 = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM runtime_checkpoints WHERE run_id=?",
            vec![run.id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert!(
        count >= 3,
        "expected checkpoints across attempt, activation, and settle work; got {count}"
    );
    let latest = store
        .load_latest_runtime_checkpoint(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.schema_version, 1);
}

fn start(revision_id: &str, key: &str) -> StartRunCommand {
    StartRunCommand {
        graph_revision_id: revision_id.into(),
        input: json!({"message":"checkpoint"}),
        context: RunContextCommand::Temporary,
        deadline_at: None,
        idempotency_key: key.into(),
    }
}
