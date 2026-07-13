use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::runtime::{RunContextCommand, StartRunCommand};

use crate::{
    StorageError,
    graph::helpers::sql,
    runtime::{Event, append_event},
};

use super::{applied_graph, store};

#[tokio::test]
async fn checkpoint_gates_noncritical_compaction_and_preserves_recovery() {
    let store = store().await;
    let revision = applied_graph(&store, "event-compaction").await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"compact"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "event-compaction-run".into(),
        })
        .await
        .unwrap();
    let debug_seq = append_event(
        &store.db,
        Event {
            run_id: &run.id,
            event_type: "runtime.debug.sample",
            importance: "debug",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1,"detail":"discardable"}),
            now: 10,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        store
            .compact_run_events(&run.id, debug_seq as u64, 11, 10)
            .await,
        Err(StorageError::Conflict("runtime_checkpoint_required"))
    ));
    let checkpoint = store.create_runtime_checkpoint(&run.id, 12).await.unwrap();
    assert_eq!(checkpoint.through_seq, debug_seq as u64);
    let report = store
        .compact_run_events(&run.id, checkpoint.through_seq, 13, 10)
        .await
        .unwrap();
    assert_eq!(report.compacted, 1);
    assert_eq!(report.checkpoint_id, checkpoint.id);
    assert_eq!(count(&store, "run_events", "runtime.debug.sample").await, 0);
    assert_eq!(count(&store, "run_events", "run.created").await, 1);
    assert_eq!(
        count(&store, "run_event_compactions", "runtime.debug.sample").await,
        1
    );
    assert_eq!(
        store
            .compact_run_events(&run.id, checkpoint.through_seq, 14, 10)
            .await
            .unwrap()
            .compacted,
        0
    );
    let recovered = store.recover_runtime_runs().await.unwrap();
    assert_eq!(recovered[0].checkpoint_id, checkpoint.id);
    assert_eq!(recovered[0].replayed_event_count, 1);
}

async fn count(store: &crate::SqliteStore, table: &str, event_type: &str) -> i64 {
    let statement = format!("SELECT COUNT(*) AS count FROM {table} WHERE event_type=?");
    store
        .db
        .query_one_raw(sql(&statement, vec![event_type.into()]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
