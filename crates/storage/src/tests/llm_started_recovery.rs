use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::EffectClassification,
    llm::{EffectAttemptFence, LlmLogicalCallStatus, StartModelCallCommand},
};

use crate::{
    SqliteStore,
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_ledger::{checkpoint, now_ms, prepare_command, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn expired_started_pure_effect_becomes_retry_ready_reconcile() {
    let store = store().await;
    let setup = prepare_started(&store, EffectClassification::Pure).await;
    assert_eq!(store.recover_expired_leases(setup.now).await.unwrap(), 1);
    let row = load_projection(&store).await;
    assert_eq!(row.attempt, "outcome_unknown");
    assert_eq!(row.effect, "pending");
    assert_eq!(row.model, "retry_ready");
    assert_eq!(row.node_attempt, "failed");
    assert_eq!(row.node_instance, "ready");
    let replacement = store
        .db
        .query_one(sql(
            "SELECT invocation_kind, status FROM node_attempts WHERE node_instance_id = ? AND attempt_no = 2",
            vec![setup.node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        replacement
            .try_get::<String>("", "invocation_kind")
            .unwrap(),
        "reconcile"
    );
    assert_eq!(
        replacement.try_get::<String>("", "status").unwrap(),
        "queued"
    );
}

#[tokio::test]
async fn expired_started_non_idempotent_effect_opens_resolution_wait() {
    let store = store().await;
    let setup = prepare_started(&store, EffectClassification::NonIdempotent).await;
    assert_eq!(store.recover_expired_leases(setup.now).await.unwrap(), 1);
    let row = load_projection(&store).await;
    assert_eq!(row.attempt, "outcome_unknown");
    assert_eq!(row.effect, "outcome_unknown");
    assert_eq!(row.model, "outcome_unknown");
    assert_eq!(row.node_attempt, "waiting");
    assert_eq!(row.node_instance, "waiting");
    let wait = store
        .db
        .query_one(sql(
            "SELECT w.status AS wait_status, wb.status AS blocker_status FROM node_waits w JOIN wait_blockers wb ON wb.wait_id = w.id WHERE wb.blocker_kind = 'effect' AND wb.blocker_id = 'effect-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wait.try_get::<String>("", "wait_status").unwrap(), "open");
    assert_eq!(
        wait.try_get::<String>("", "blocker_status").unwrap(),
        "open"
    );
    let replacement = store
        .db
        .query_one(sql(
            "SELECT 1 AS present FROM node_attempts WHERE node_instance_id = ? AND attempt_no = 2",
            vec![setup.node_instance_id.into()],
        ))
        .await
        .unwrap();
    assert!(replacement.is_none());
}

struct StartedSetup {
    node_instance_id: String,
    now: i64,
}

async fn prepare_started(
    store: &SqliteStore,
    classification: EffectClassification,
) -> StartedSetup {
    let claimed = prepare_running_llm_attempt(store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_object_id = store
        .db
        .query_one(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "execution_snapshot_object_id")
        .unwrap();
    let transcript_ref = put_inline_object(
        &store.db,
        &canonical::to_vec(&json!({"schemaVersion":1,"items":[]})).unwrap(),
        now,
    )
    .await
    .unwrap();
    let mut command = prepare_command(
        &claimed,
        &snapshot,
        checkpoint(
            &claimed,
            &snapshot_object_id,
            &transcript_ref,
            LlmLogicalCallStatus::Prepared,
            None,
        ),
    );
    command.effect_classification = classification;
    store.prepare_model_call(command, now - 3).await.unwrap();
    let fence = EffectAttemptFence {
        invoking_node_attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
    };
    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence,
                provider_request_id: Some("provider-request-expired".into()),
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
                ),
            },
            now - 2,
        )
        .await
        .unwrap();
    store
        .db
        .execute(sql(
            "UPDATE node_attempts SET lease_until = ? WHERE id = ?",
            vec![(now - 1).into(), claimed.attempt_id.into()],
        ))
        .await
        .unwrap();
    StartedSetup {
        node_instance_id: claimed.node_instance_id,
        now,
    }
}

struct Projection {
    attempt: String,
    effect: String,
    model: String,
    node_attempt: String,
    node_instance: String,
}

async fn load_projection(store: &SqliteStore) -> Projection {
    let row = store.db.query_one(sql(
        "SELECT ea.status AS attempt_status, e.status AS effect_status, mc.status AS model_status, a.status AS node_attempt_status, ni.status AS node_instance_status FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id JOIN model_calls mc ON mc.id = e.model_call_id JOIN node_attempts a ON a.id = ea.invoking_node_attempt_id JOIN node_instances ni ON ni.id = e.node_instance_id WHERE ea.id = 'effect-attempt-1'",
        vec![],
    )).await.unwrap().unwrap();
    Projection {
        attempt: row.try_get("", "attempt_status").unwrap(),
        effect: row.try_get("", "effect_status").unwrap(),
        model: row.try_get("", "model_status").unwrap(),
        node_attempt: row.try_get("", "node_attempt_status").unwrap(),
        node_instance: row.try_get("", "node_instance_status").unwrap(),
    }
}
