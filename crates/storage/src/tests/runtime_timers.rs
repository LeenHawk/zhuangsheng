use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::graph::{ApplyGraphCommand, UpdateGraphDraftCommand},
    graph::{RefreshReadSet, RetryPolicy},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler, SchedulerWork},
};

use crate::{
    StorageError,
    graph::helpers::{now_ms, sql},
};

use super::{graph, run_draft, store};

#[tokio::test]
async fn run_deadline_fences_all_work() {
    let store = Arc::new(store().await);
    let revision = retry_graph(&store, "run-deadline", None).await;
    let deadline = now_ms() + 1_000;
    let run = store
        .start_run(start(&revision.id, "deadline-run", Some(deadline)))
        .await
        .unwrap();
    Scheduler::new(store.clone(), "deadline-worker")
        .run_until_idle(deadline + 1, 8)
        .await
        .unwrap();
    let failed = store.get_run(&run.id).await.unwrap();
    assert_eq!(failed.status, RunStatus::Failed);
    assert_eq!(failed.control_epoch, 1);
    assert_eq!(count_where(&store, "run_output_values", "1=1").await, 0);
}

#[tokio::test]
async fn attempt_timeout_waits_for_deterministic_retry_timer() {
    let store = Arc::new(store().await);
    let revision = retry_graph(&store, "attempt-retry", Some(policy(1))).await;
    let run = store
        .start_run(start(&revision.id, "retry-run", None))
        .await
        .unwrap();
    let now = now_ms();
    let SchedulerWork::Attempt(claimed) = store
        .claim_next_work("timeout-worker", now, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected input attempt")
    };
    store.mark_attempt_running(&claimed, now).await.unwrap();
    assert_eq!(store.process_due_timers(now + 11).await.unwrap(), 1);
    assert_eq!(
        count_where(&store, "node_attempts", "status = 'timed_out'").await,
        1
    );
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Waiting
    );
    let retry = store
        .db
        .query_one_raw(sql(
            "SELECT due_at, status FROM runtime_timers WHERE kind = 'retry'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.try_get::<i64>("", "due_at").unwrap(), now + 61);
    assert_eq!(retry.try_get::<String>("", "status").unwrap(), "pending");
    assert_eq!(store.process_due_timers(now + 60).await.unwrap(), 0);
    assert_eq!(store.process_due_timers(now + 61).await.unwrap(), 1);

    Scheduler::new(store.clone(), "retry-worker")
        .run_until_idle(now + 61, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(count_where(&store, "node_attempts", "1=1").await, 3);

    let late = store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: claimed.wakeup_id,
                attempt_id: claimed.attempt_id.clone(),
                worker_id: claimed.worker_id,
                lease_fence: claimed.lease_fence,
                run_control_epoch: claimed.run_control_epoch,
                result_idempotency_key: format!("late-timeout:{}", claimed.attempt_id),
                result: BuiltinResult::Completed {
                    outputs: claimed.inputs,
                },
            },
            now + 62,
        )
        .await
        .unwrap_err();
    assert!(matches!(late, StorageError::Conflict("attempt_fence")));
}

#[tokio::test]
async fn retry_budget_exhaustion_fails_run() {
    let store = Arc::new(store().await);
    let revision = retry_graph(&store, "retry-exhausted", Some(policy(1))).await;
    let run = store
        .start_run(start(&revision.id, "exhaust-run", None))
        .await
        .unwrap();
    let now = now_ms();
    let SchedulerWork::Attempt(first) = store
        .claim_next_work("timeout-1", now, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected first attempt")
    };
    store.mark_attempt_running(&first, now).await.unwrap();
    store.process_due_timers(now + 11).await.unwrap();
    store.process_due_timers(now + 61).await.unwrap();
    let SchedulerWork::Attempt(retry) = store
        .claim_next_work("timeout-2", now + 61, now + 30_061)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected retry attempt")
    };
    store.mark_attempt_running(&retry, now + 61).await.unwrap();
    store.process_due_timers(now + 72).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Failed
    );
    assert_eq!(
        count_where(&store, "node_attempts", "status = 'timed_out'").await,
        2
    );
    assert_eq!(
        count_where(&store, "runtime_timers", "kind = 'retry'").await,
        1
    );
}

async fn retry_graph(
    store: &crate::SqliteStore,
    key: &str,
    retry_policy: Option<RetryPolicy>,
) -> zhuangsheng_core::application::graph::GraphRevisionView {
    let graph = graph(store, &format!("create-{key}")).await;
    let initial = store.get_graph_draft(&graph.id).await.unwrap();
    let mut document = run_draft(&graph.id);
    document.nodes[0].timeout_ms = Some(10);
    document.nodes[0].retry_policy = retry_policy;
    let draft = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: initial.revision_token,
            document,
            idempotency_key: format!("draft-{key}"),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.id,
            expected_revision_token: draft.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap()
}

fn policy(max_retries: u64) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        retry_on: vec!["node_timeout".into()],
        initial_backoff_ms: 50,
        multiplier_micros: 1_000_000,
        max_backoff_ms: 50,
        jitter_ratio_micros: 0,
        refresh_read_set: RefreshReadSet::Never,
    }
}

fn start(revision_id: &str, key: &str, deadline_at: Option<i64>) -> StartRunCommand {
    StartRunCommand {
        graph_revision_id: revision_id.into(),
        input: json!({"message":"timer"}),
        context: RunContextCommand::Temporary,
        deadline_at,
        idempotency_key: key.into(),
    }
}

async fn count_where(store: &crate::SqliteStore, table: &str, predicate: &str) -> i64 {
    store
        .db
        .query_one_raw(sql(
            &format!("SELECT COUNT(*) AS count FROM {table} WHERE {predicate}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
