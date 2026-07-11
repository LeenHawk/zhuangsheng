use std::sync::Arc;

use serde_json::json;
use zhuangsheng_core::{
    runtime::{RunContextCommand, RunControlCommand, RunStatus, StartRunCommand},
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler, SchedulerWork},
};

use crate::{StorageError, graph::helpers::now_ms};

use super::{applied_graph, store};

#[tokio::test]
async fn interrupt_resume_and_historical_replay_are_durable() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "control-resume").await;
    let run = store
        .start_run(start(&revision.id, "control-run"))
        .await
        .unwrap();
    let interrupt = control(&run.id, 0, "interrupt-1");
    let interrupted = store.request_interrupt(interrupt.clone()).await.unwrap();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(interrupted.control_epoch, 1);

    let resumed = store
        .resume_interrupted(control(&run.id, 1, "resume-1"))
        .await
        .unwrap();
    assert_eq!(resumed.status, RunStatus::Running);
    assert_eq!(resumed.control_epoch, 2);
    Scheduler::new(store.clone(), "resume-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );

    let replay = store.request_interrupt(interrupt.clone()).await.unwrap();
    assert_eq!(replay, interrupted);
    let conflict = store
        .request_interrupt(RunControlCommand {
            reason: Some("different".into()),
            ..interrupt
        })
        .await
        .unwrap_err();
    assert!(matches!(conflict, StorageError::IdempotencyConflict));
}

#[tokio::test]
async fn running_attempt_drains_under_old_epoch_then_stops() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "control-drain").await;
    let run = store
        .start_run(start(&revision.id, "drain-run"))
        .await
        .unwrap();
    let now = now_ms();
    let SchedulerWork::Attempt(claimed) = store
        .claim_next_work("drain-worker", now, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected input attempt")
    };
    store.mark_attempt_running(&claimed, now).await.unwrap();
    let interrupting = store
        .request_interrupt(control(&run.id, 0, "interrupt-drain"))
        .await
        .unwrap();
    assert_eq!(interrupting.status, RunStatus::Interrupting);
    assert_eq!(interrupting.control_epoch, 1);

    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: claimed.wakeup_id,
                attempt_id: claimed.attempt_id.clone(),
                worker_id: claimed.worker_id,
                lease_fence: claimed.lease_fence,
                run_control_epoch: claimed.run_control_epoch,
                result_idempotency_key: format!("drain-result:{}", claimed.attempt_id),
                result: BuiltinResult::Completed {
                    outputs: claimed.inputs,
                },
            },
            now_ms(),
        )
        .await
        .unwrap();
    let interrupted = store.get_run(&run.id).await.unwrap();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(interrupted.control_epoch, 1);

    store
        .resume_interrupted(control(&run.id, 1, "resume-drain"))
        .await
        .unwrap();
    Scheduler::new(store.clone(), "post-drain-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
}

#[tokio::test]
async fn cancel_fences_running_attempt_and_all_future_work() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "control-cancel").await;
    let run = store
        .start_run(start(&revision.id, "cancel-run"))
        .await
        .unwrap();
    let now = now_ms();
    let SchedulerWork::Attempt(claimed) = store
        .claim_next_work("cancelled-worker", now, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected input attempt")
    };
    store.mark_attempt_running(&claimed, now).await.unwrap();
    let cancel = control(&run.id, 0, "cancel-1");
    let cancelled = store.request_cancel(cancel.clone()).await.unwrap();
    assert_eq!(cancelled.status, RunStatus::Cancelled);
    assert_eq!(cancelled.control_epoch, 1);

    let late = store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: claimed.wakeup_id,
                attempt_id: claimed.attempt_id.clone(),
                worker_id: claimed.worker_id,
                lease_fence: claimed.lease_fence,
                run_control_epoch: claimed.run_control_epoch,
                result_idempotency_key: format!("late-cancel:{}", claimed.attempt_id),
                result: BuiltinResult::Completed {
                    outputs: claimed.inputs,
                },
            },
            now_ms(),
        )
        .await
        .unwrap_err();
    assert!(matches!(late, StorageError::Conflict("attempt_fence")));
    assert_eq!(
        Scheduler::new(store.clone(), "after-cancel")
            .run_until_idle(now_ms(), 8)
            .await
            .unwrap(),
        0
    );
    assert_eq!(store.request_cancel(cancel).await.unwrap(), cancelled);
    assert!(matches!(
        store
            .request_interrupt(control(&run.id, 1, "interrupt-cancelled"))
            .await
            .unwrap_err(),
        StorageError::Conflict("run_lifecycle")
    ));
}

#[tokio::test]
async fn interrupting_run_recovers_expired_drain_before_resume() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "control-drain-expiry").await;
    let run = store
        .start_run(start(&revision.id, "drain-expiry-run"))
        .await
        .unwrap();
    let now = now_ms();
    let SchedulerWork::Attempt(claimed) = store
        .claim_next_work("lost-drain-worker", now, now + 1)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected input attempt")
    };
    store.mark_attempt_running(&claimed, now).await.unwrap();
    assert_eq!(
        store
            .request_interrupt(control(&run.id, 0, "interrupt-expiring-drain"))
            .await
            .unwrap()
            .status,
        RunStatus::Interrupting
    );
    store.recover_expired_leases(now + 2).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Interrupted
    );
    store
        .resume_interrupted(control(&run.id, 1, "resume-expired-drain"))
        .await
        .unwrap();
    Scheduler::new(store.clone(), "recovered-drain-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
}

fn start(revision_id: &str, key: &str) -> StartRunCommand {
    StartRunCommand {
        graph_revision_id: revision_id.into(),
        input: json!({"message":"control"}),
        context: RunContextCommand::Temporary,
        deadline_at: None,
        idempotency_key: key.into(),
    }
}

fn control(run_id: &str, expected_epoch: u64, key: &str) -> RunControlCommand {
    RunControlCommand {
        run_id: run_id.into(),
        expected_epoch,
        idempotency_key: key.into(),
        reason: None,
    }
}
