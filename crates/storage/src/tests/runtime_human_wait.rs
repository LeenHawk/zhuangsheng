use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    runtime::{
        RunContextCommand, RunStatus, StartRunCommand, SubmitWaitResponseCommand, WaitKind,
        WaitResponsePayload,
    },
    scheduler::{
        BuiltinResult, ExternalWaitRequest, FinalizeAttemptCommand, Scheduler, SchedulerWork,
        WaitTimeoutPolicy,
    },
};

use crate::{
    StorageError,
    graph::helpers::{now_ms, sql},
};

use super::{applied_graph, schema};

#[tokio::test]
async fn human_wait_survives_restart_validates_and_resumes_once() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", file.path().display());
    let deadline_base = now_ms();
    let (run_id, wait_id, now) = {
        let store = Arc::new(crate::SqliteStore::connect(&url).await.unwrap());
        let revision = applied_graph(&store, "human-wait").await;
        let run = store
            .start_run(StartRunCommand {
                graph_revision_id: revision.id,
                input: json!({"message":"before wait"}),
                context: RunContextCommand::Temporary,
                deadline_at: Some(deadline_base + 60_000),
                idempotency_key: "human-wait-run".into(),
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
        store.finalize_attempt(FinalizeAttemptCommand {
            wakeup_id: attempt.wakeup_id,
            attempt_id: attempt.attempt_id.clone(),
            worker_id: attempt.worker_id,
            lease_fence: attempt.lease_fence,
            run_control_epoch: attempt.run_control_epoch,
            result_idempotency_key: format!("wait:{}", attempt.attempt_id),
            result: BuiltinResult::Waiting {
                wait: Box::new(ExternalWaitRequest {
                    kind: WaitKind::HumanResponse,
                    request: json!({"schemaVersion":1,"kind":"human_response","title":"Choose a path"}),
                    response_schema: Some(schema(json!({
                        "type":"object",
                        "properties":{"choice":{"enum":["left","right"]}},
                        "required":["choice"],
                        "additionalProperties":false
                    }))),
                    correlation_key: Some("story-choice-1".into()),
                    deadline_at: Some(now + 50_000),
                    on_timeout: WaitTimeoutPolicy::Fail,
                }),
                continuation: json!({"schemaVersion":1,"step":"after_choice"}),
            },
        }, now + 1).await.unwrap();
        Scheduler::new(store.clone(), "settle-worker")
            .run_until_idle(now + 2, 8)
            .await
            .unwrap();
        assert_eq!(
            store.get_run(&run.id).await.unwrap().status,
            RunStatus::Waiting
        );
        let waits = store.list_open_waits(&run.id).await.unwrap();
        assert_eq!(waits.len(), 1);
        assert!(waits[0].response_schema.is_some());
        assert!(waits[0].response_schema_compilation.is_some());
        (run.id, waits[0].id.clone(), now)
    };

    let store = Arc::new(crate::SqliteStore::connect(&url).await.unwrap());
    let invalid = SubmitWaitResponseCommand {
        wait_id: wait_id.clone(),
        delivery_id: "human-delivery-1".into(),
        actor_kind: "human".into(),
        actor_id: Some("local-user".into()),
        payload: WaitResponsePayload::Value {
            value: json!({"choice":"up"}),
        },
    };
    assert!(matches!(
        store.submit_wait_response(invalid, now + 3).await,
        Err(StorageError::Domain(_))
    ));
    assert_eq!(delivery_count(&store).await, 0);

    let valid = SubmitWaitResponseCommand {
        wait_id: wait_id.clone(),
        delivery_id: "human-delivery-1".into(),
        actor_kind: "human".into(),
        actor_id: Some("local-user".into()),
        payload: WaitResponsePayload::Value {
            value: json!({"choice":"left"}),
        },
    };
    let first = store
        .submit_wait_response(valid.clone(), now + 4)
        .await
        .unwrap();
    assert!(!first.replayed);
    assert!(
        store
            .submit_wait_response(valid, now + 5)
            .await
            .unwrap()
            .replayed
    );
    assert_eq!(delivery_count(&store).await, 1);
    let SchedulerWork::Attempt(resume) = store
        .claim_next_work("resume-worker", now + 6, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected resume attempt")
    };
    let snapshot = resume
        .wait_resume
        .as_ref()
        .expect("durable wait resume snapshot");
    assert_eq!(snapshot.wait_id, wait_id);
    assert_eq!(
        snapshot.continuation,
        json!({"schemaVersion":1,"step":"after_choice"})
    );
    assert_eq!(
        snapshot.response,
        json!({"schemaVersion":1,"kind":"value","value":{"choice":"left"}})
    );
    store.mark_attempt_running(&resume, now + 6).await.unwrap();
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: resume.wakeup_id,
                attempt_id: resume.attempt_id.clone(),
                worker_id: resume.worker_id,
                lease_fence: resume.lease_fence,
                run_control_epoch: resume.run_control_epoch,
                result_idempotency_key: format!("result:{}", resume.attempt_id),
                result: BuiltinResult::Completed {
                    outputs: resume.inputs,
                },
            },
            now + 7,
        )
        .await
        .unwrap();
    Scheduler::new(store.clone(), "finish-worker")
        .run_until_idle(now + 8, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run_id).await.unwrap().status,
        RunStatus::Completed
    );
    assert!(store.list_open_waits(&run_id).await.unwrap().is_empty());
}

async fn delivery_count(store: &crate::SqliteStore) -> i64 {
    store
        .db
        .query_one_raw(sql("SELECT COUNT(*) AS count FROM wait_deliveries", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
