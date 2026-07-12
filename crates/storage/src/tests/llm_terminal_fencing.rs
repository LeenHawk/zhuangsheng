use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{EffectAttemptFence, LlmLogicalCallStatus, LlmLoopCheckpoint, StartModelCallCommand},
    runtime::RunControlCommand,
    scheduler::ClaimedAttempt,
};

use crate::{
    SqliteStore,
    graph::helpers::{load_object_json, put_inline_object, sql},
    tests::{
        llm_ledger::{checkpoint, now_ms, prepare_command, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn cancelling_prepared_effect_proves_not_started_and_cancels_owner() {
    let store = store().await;
    let setup = prepare_effect(&store).await;
    store
        .request_cancel(cancel_command(&setup.claimed, "cancel-prepared-effect"))
        .await
        .unwrap();
    assert_terminal_projection(
        &store,
        "superseded_before_start",
        "cancelled_before_start",
        "cancelled_before_start",
        "run_terminal_cancel_before_start",
        LlmLogicalCallStatus::CancelledBeforeStart,
        &setup.claimed.node_instance_id,
    )
    .await;
}

#[tokio::test]
async fn cancelling_started_effect_records_unknown_fact_and_abandons_owner() {
    let store = store().await;
    let setup = prepare_effect(&store).await;
    let fence = EffectAttemptFence {
        invoking_node_attempt_id: setup.claimed.attempt_id.clone(),
        worker_id: setup.claimed.worker_id.clone(),
        lease_fence: setup.claimed.lease_fence,
        run_control_epoch: setup.claimed.run_control_epoch,
    };
    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence,
                provider_request_id: Some("provider-request-started".into()),
                checkpoint: checkpoint(
                    &setup.claimed,
                    &setup.snapshot_object_id,
                    &setup.transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
                ),
            },
            setup.now + 1,
        )
        .await
        .unwrap();
    store
        .request_cancel(cancel_command(&setup.claimed, "cancel-started-effect"))
        .await
        .unwrap();
    assert_terminal_projection(
        &store,
        "outcome_unknown",
        "abandoned_unknown",
        "abandoned_unknown",
        "run_terminal_abandon",
        LlmLogicalCallStatus::AbandonedUnknown,
        &setup.claimed.node_instance_id,
    )
    .await;
}

struct EffectSetup {
    claimed: ClaimedAttempt,
    snapshot_object_id: String,
    transcript_ref: String,
    now: i64,
}

async fn prepare_effect(store: &SqliteStore) -> EffectSetup {
    let claimed = prepare_running_llm_attempt(store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_object_id = store
        .db
        .query_one_raw(sql(
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
    store
        .prepare_model_call(
            prepare_command(
                &claimed,
                &snapshot,
                checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Prepared,
                    None,
                ),
            ),
            now,
        )
        .await
        .unwrap();
    EffectSetup {
        claimed,
        snapshot_object_id,
        transcript_ref,
        now,
    }
}

fn cancel_command(claimed: &ClaimedAttempt, key: &str) -> RunControlCommand {
    RunControlCommand {
        run_id: claimed.run_id.clone(),
        expected_epoch: claimed.run_control_epoch,
        idempotency_key: key.into(),
        reason: Some("terminal fencing test".into()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn assert_terminal_projection(
    store: &SqliteStore,
    attempt_status: &str,
    effect_status: &str,
    model_status: &str,
    resolution_kind: &str,
    checkpoint_status: LlmLogicalCallStatus,
    node_instance_id: &str,
) {
    let row = store.db.query_one_raw(sql(
        "SELECT ea.status AS attempt_status, e.status AS effect_status, mc.status AS model_status, er.resolution_kind FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id JOIN model_calls mc ON mc.id = e.model_call_id JOIN effect_resolutions er ON er.effect_attempt_id = ea.id WHERE ea.id = 'effect-attempt-1'",
        vec![],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        attempt_status
    );
    assert_eq!(
        row.try_get::<String>("", "effect_status").unwrap(),
        effect_status
    );
    assert_eq!(
        row.try_get::<String>("", "model_status").unwrap(),
        model_status
    );
    assert_eq!(
        row.try_get::<String>("", "resolution_kind").unwrap(),
        resolution_kind
    );
    let checkpoint_row = store
        .db
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let checkpoint: LlmLoopCheckpoint = load_object_json(
        &store.db,
        &checkpoint_row
            .try_get::<String>("", "checkpoint_object_id")
            .unwrap(),
    )
    .await
    .unwrap();
    assert!(checkpoint.checksum_is_valid());
    assert_eq!(
        checkpoint.active_model_effect.unwrap().status,
        checkpoint_status
    );
}
