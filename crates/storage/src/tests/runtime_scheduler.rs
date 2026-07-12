use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler, SchedulerWork},
};

use crate::{
    StorageError,
    graph::helpers::{load_object_json, now_ms, sql},
};

use super::{applied_graph, store};

#[tokio::test]
async fn fifo_input_to_output_completes_once() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "fifo").await;
    let run = store
        .start_run(start(&revision.id, "fifo-run", "hello"))
        .await
        .unwrap();
    let scheduler = Scheduler::new(store.clone(), "worker-fifo");
    let steps = scheduler.run_until_idle(now_ms(), 64).await.unwrap();
    assert!(
        steps >= 4,
        "expected attempt, activation, output, and settle work"
    );

    let completed = store.get_run(&run.id).await.unwrap();
    assert_eq!(completed.status, RunStatus::Completed);
    assert_eq!(
        completed.output_commit_id,
        Some(completed.input_commit_id.clone())
    );
    assert_eq!(scheduler.run_until_idle(now_ms(), 8).await.unwrap(), 0);

    assert_eq!(count(&store, "node_instances").await, 2);
    assert_eq!(count(&store, "node_attempts").await, 2);
    assert_eq!(count(&store, "edge_queue_values").await, 1);
    assert_eq!(count(&store, "run_output_values").await, 1);
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT value_object_id FROM run_output_values WHERE run_id = ? AND output_key = 'reply'",
            vec![run.id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let object_id: String = row.try_get("", "value_object_id").unwrap();
    let value: Value = load_object_json(&store.db, &object_id).await.unwrap();
    assert_eq!(value, json!("hello"));

    let sequences = store
        .db
        .query_all_raw(sql(
            "SELECT seq FROM run_events WHERE run_id = ? ORDER BY seq",
            vec![completed.id.into()],
        ))
        .await
        .unwrap();
    let sequences: Vec<i64> = sequences
        .iter()
        .map(|row| row.try_get("", "seq").unwrap())
        .collect();
    assert_eq!(sequences, (1..=sequences.len() as i64).collect::<Vec<_>>());
}

#[tokio::test]
async fn expired_running_lease_retries_and_rejects_late_result() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "lease").await;
    let run = store
        .start_run(start(&revision.id, "lease-run", "recover"))
        .await
        .unwrap();
    let now = now_ms();
    let work = store
        .claim_next_work("dead-worker", now, now + 1)
        .await
        .unwrap()
        .unwrap();
    let SchedulerWork::Attempt(claimed) = work else {
        panic!("expected initial attempt")
    };
    store.mark_attempt_running(&claimed, now).await.unwrap();

    let scheduler = Scheduler::new(store.clone(), "recovery-worker");
    scheduler.run_until_idle(now + 2, 64).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(count(&store, "run_output_values").await, 1);
    assert_eq!(count(&store, "node_attempts").await, 3);

    let late = store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: claimed.wakeup_id,
                attempt_id: claimed.attempt_id.clone(),
                worker_id: claimed.worker_id,
                lease_fence: claimed.lease_fence,
                run_control_epoch: claimed.run_control_epoch,
                result_idempotency_key: format!("late:{}", claimed.attempt_id),
                result: BuiltinResult::Completed {
                    outputs: claimed.inputs,
                },
            },
            now + 3,
        )
        .await
        .unwrap_err();
    assert!(matches!(late, StorageError::Conflict("attempt_fence")));
    assert_eq!(count(&store, "run_output_values").await, 1);
}

#[tokio::test]
async fn restart_after_finalize_uses_durable_wakeup_without_duplicate_enqueue() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", file.path().display());
    let (run_id, now) = {
        let store = Arc::new(crate::SqliteStore::connect(&url).await.unwrap());
        let revision = applied_graph(&store, "finalize-restart").await;
        let run = store
            .start_run(start(&revision.id, "finalize-restart", "durable"))
            .await
            .unwrap();
        let now = now_ms();
        let SchedulerWork::Attempt(claimed) = store
            .claim_next_work("pre-crash-worker", now, now + 30_000)
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("expected input attempt")
        };
        store.mark_attempt_running(&claimed, now).await.unwrap();
        let command = FinalizeAttemptCommand {
            wakeup_id: claimed.wakeup_id.clone(),
            attempt_id: claimed.attempt_id.clone(),
            worker_id: claimed.worker_id.clone(),
            lease_fence: claimed.lease_fence,
            run_control_epoch: claimed.run_control_epoch,
            result_idempotency_key: format!(
                "result:{}:{}",
                claimed.attempt_id, claimed.lease_fence
            ),
            result: BuiltinResult::Completed {
                outputs: claimed.inputs.clone(),
            },
        };
        store.finalize_attempt(command.clone(), now).await.unwrap();
        store.finalize_attempt(command, now).await.unwrap();
        assert_eq!(count(&store, "edge_queue_values").await, 1);
        (run.id, now)
    };

    let reopened = Arc::new(crate::SqliteStore::connect(&url).await.unwrap());
    Scheduler::new(reopened.clone(), "post-crash-worker")
        .run_until_idle(now + 1, 64)
        .await
        .unwrap();
    assert_eq!(
        reopened.get_run(&run_id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(count(&reopened, "edge_queue_values").await, 1);
    assert_eq!(count(&reopened, "run_output_values").await, 1);
}

fn start(revision_id: &str, key: &str, message: &str) -> StartRunCommand {
    StartRunCommand {
        graph_revision_id: revision_id.into(),
        input: json!({"message":message}),
        context: RunContextCommand::Temporary,
        deadline_at: None,
        idempotency_key: key.into(),
    }
}

async fn count(store: &crate::SqliteStore, table: &str) -> i64 {
    store
        .db
        .query_one_raw(sql(
            &format!("SELECT COUNT(*) AS count FROM {table}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
