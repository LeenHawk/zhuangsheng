use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    runtime::{RunContextCommand, RunStatus, StartRunCommand, WaitKind},
    scheduler::{
        BuiltinResult, ExternalWaitRequest, FinalizeAttemptCommand, Scheduler, SchedulerWork,
        WaitTimeoutPolicy,
    },
};

use crate::graph::helpers::{now_ms, sql};

use super::{applied_graph, schema, store};

#[tokio::test]
async fn wait_deadline_resumes_with_a_durable_timeout_value() {
    let store = Arc::new(store().await);
    let revision = applied_graph(&store, "wait-timeout").await;
    let base = now_ms();
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"timeout"}),
            context: RunContextCommand::Temporary,
            deadline_at: Some(base + 60_000),
            idempotency_key: "wait-timeout-run".into(),
        })
        .await
        .unwrap();
    let now = now_ms();
    let SchedulerWork::Attempt(attempt) = store
        .claim_next_work("wait-worker", now, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected input attempt")
    };
    store.mark_attempt_running(&attempt, now).await.unwrap();
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: attempt.wakeup_id,
                attempt_id: attempt.attempt_id.clone(),
                worker_id: attempt.worker_id,
                lease_fence: attempt.lease_fence,
                run_control_epoch: attempt.run_control_epoch,
                result_idempotency_key: format!("wait:{}", attempt.attempt_id),
                result: BuiltinResult::Waiting {
                    wait: Box::new(ExternalWaitRequest {
                        kind: WaitKind::HumanResponse,
                        request: json!({"schemaVersion":1,"kind":"human_response"}),
                        response_schema: Some(schema(json!({
                            "type":"object",
                            "properties":{"timedOut":{"const":true}},
                            "required":["timedOut"],
                            "additionalProperties":false
                        }))),
                        correlation_key: None,
                        deadline_at: Some(now + 10),
                        on_timeout: WaitTimeoutPolicy::ResumeWithTimeout,
                    }),
                    continuation: json!({"schemaVersion":1,"step":"timeout"}),
                },
            },
            now + 1,
        )
        .await
        .unwrap();
    Scheduler::new(store.clone(), "settle-worker")
        .run_until_idle(now + 2, 8)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Waiting
    );
    assert_eq!(store.process_due_timers(now + 11).await.unwrap(), 1);
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Running
    );

    let SchedulerWork::Attempt(resume) = store
        .claim_next_work("resume-worker", now + 12, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected timeout resume attempt")
    };
    assert_eq!(
        resume.wait_resume.as_ref().unwrap().response,
        json!({
            "schemaVersion":1,"kind":"value","value":{"timedOut":true}
        })
    );
    let wait = store
        .db
        .query_one_raw(sql(
            "SELECT status,accepted_delivery_id FROM node_waits WHERE run_id=?",
            vec![run.id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wait.try_get::<String>("", "status").unwrap(), "expired");
    assert!(
        wait.try_get::<String>("", "accepted_delivery_id")
            .unwrap()
            .starts_with("timeout:")
    );
}
