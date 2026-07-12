use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectAttemptFence, EffectResolutionActorKind, EffectResolutionKind,
        FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint, ModelCallEffectOutcome,
        ResolveEffectUnknownCommand, StartModelCallCommand,
    },
    scheduler::ClaimedAttempt,
};

use crate::{
    SqliteStore, StorageError,
    graph::helpers::{load_object_json, put_inline_object, sql},
    tests::{
        llm_ledger::{checkpoint, now_ms, prepare_command, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn unknown_model_outcome_opens_durable_wait_and_finish_replays() {
    let store = store().await;
    let setup = prepare_unknown(&store).await;
    replay_unknown_finish(&store, &setup).await;
    let row = store
        .db
        .query_one(sql(
            "SELECT w.status AS wait_status, wb.status AS blocker_status, a.status AS attempt_status, ni.status AS instance_status, c.open_waits, (SELECT COUNT(*) FROM scheduler_wakeups sw WHERE sw.run_id = w.run_id AND sw.status = 'claimed') AS claimed_wakeups, (SELECT COUNT(*) FROM runtime_timers rt WHERE rt.node_attempt_id = a.id AND rt.status = 'pending') AS pending_timers FROM node_waits w JOIN wait_blockers wb ON wb.wait_id = w.id JOIN node_attempts a ON a.id = w.node_attempt_id JOIN node_instances ni ON ni.id = w.node_instance_id JOIN run_execution_counters c ON c.run_id = w.run_id WHERE wb.blocker_kind = 'effect' AND wb.blocker_id = 'effect-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "wait_status").unwrap(), "open");
    assert_eq!(row.try_get::<String>("", "blocker_status").unwrap(), "open");
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        "waiting"
    );
    assert_eq!(
        row.try_get::<String>("", "instance_status").unwrap(),
        "waiting"
    );
    assert_eq!(row.try_get::<i64>("", "open_waits").unwrap(), 1);
    assert_eq!(row.try_get::<i64>("", "claimed_wakeups").unwrap(), 0);
    assert_eq!(row.try_get::<i64>("", "pending_timers").unwrap(), 0);
    let checkpoint = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    assert_eq!(checkpoint.wait_ids.len(), 1);
    assert_eq!(
        checkpoint.active_model_effect.unwrap().status,
        LlmLogicalCallStatus::OutcomeUnknown
    );
}

#[tokio::test]
async fn retry_safe_resolution_preserves_unknown_fact_and_resumes() {
    let store = store().await;
    let setup = prepare_unknown(&store).await;
    let command = resolution_command(
        &setup,
        "resolution-retry-safe",
        "resolution-key-retry-safe",
        EffectResolutionKind::ConfirmFailedRetrySafe,
        json!({"reason":"provider confirmed request was not applied"}),
        None,
        None,
    );
    let resolved = store
        .resolve_effect_unknown(command.clone(), setup.now + 3)
        .await
        .unwrap();
    assert!(!resolved.replayed);
    let replayed = store
        .resolve_effect_unknown(command, setup.now + 4)
        .await
        .unwrap();
    assert!(replayed.replayed);
    let conflicting = store
        .resolve_effect_unknown(
            resolution_command(
                &setup,
                "resolution-retry-safe",
                "resolution-key-retry-safe",
                EffectResolutionKind::ConfirmFailedRetrySafe,
                json!({"reason":"different"}),
                None,
                None,
            ),
            setup.now + 5,
        )
        .await;
    assert!(matches!(
        conflicting,
        Err(StorageError::IdempotencyConflict)
    ));
    assert_projection(
        &store,
        "pending",
        "retry_ready",
        "outcome_unknown",
        "resolved",
        "satisfied",
        "ready",
    )
    .await;
    let checkpoint = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    assert_eq!(
        checkpoint.active_model_effect.unwrap().status,
        LlmLogicalCallStatus::RetryReady
    );
    assert_resume_attempt(&store, &setup.claimed.node_instance_id).await;
}

#[tokio::test]
async fn confirmed_success_binds_result_without_rewriting_attempt_fact() {
    let store = store().await;
    let setup = prepare_unknown(&store).await;
    let result_object_id = put_inline_object(
        &store.db,
        &canonical::to_vec(&json!({"text":"confirmed result"})).unwrap(),
        setup.now + 3,
    )
    .await
    .unwrap();
    store
        .resolve_effect_unknown(
            resolution_command(
                &setup,
                "resolution-success",
                "resolution-key-success",
                EffectResolutionKind::ConfirmSucceeded,
                json!({"reason":"provider lookup returned the completed response"}),
                Some(result_object_id.clone()),
                None,
            ),
            setup.now + 4,
        )
        .await
        .unwrap();
    assert_projection(
        &store,
        "succeeded",
        "completed",
        "outcome_unknown",
        "resolved",
        "satisfied",
        "ready",
    )
    .await;
    let row = store
        .db
        .query_one(sql(
            "SELECT response_object_id FROM model_calls WHERE id = 'model-call-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "response_object_id").unwrap(),
        result_object_id
    );
    let checkpoint = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    let active = checkpoint.active_model_effect.unwrap();
    assert_eq!(active.status, LlmLogicalCallStatus::Completed);
    assert_eq!(active.response_ref, Some(result_object_id));
}

#[tokio::test]
async fn abort_resolution_cancels_run_and_keeps_unknown_attempt() {
    let store = store().await;
    let setup = prepare_unknown(&store).await;
    store
        .resolve_effect_unknown(
            resolution_command(
                &setup,
                "resolution-abort",
                "resolution-key-abort",
                EffectResolutionKind::AbortRun,
                json!({"reason":"operator chose isolation over retry"}),
                None,
                None,
            ),
            setup.now + 3,
        )
        .await
        .unwrap();
    assert_projection(
        &store,
        "abandoned_unknown",
        "abandoned_unknown",
        "outcome_unknown",
        "cancelled",
        "aborted",
        "cancelled",
    )
    .await;
    let row = store
        .db
        .query_one(sql(
            "SELECT status FROM graph_runs WHERE id = ?",
            vec![setup.claimed.run_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "cancelled");
}

struct UnknownSetup {
    claimed: ClaimedAttempt,
    fence: EffectAttemptFence,
    snapshot_object_id: String,
    transcript_ref: String,
    now: i64,
}

async fn prepare_unknown(store: &SqliteStore) -> UnknownSetup {
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
                fence: fence.clone(),
                provider_request_id: Some("provider-request-unknown".into()),
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
                ),
            },
            now + 1,
        )
        .await
        .unwrap();
    store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                outcome: ModelCallEffectOutcome::OutcomeUnknown {
                    error_bytes: unknown_error(),
                },
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::OutcomeUnknown,
                    None,
                ),
            },
            now + 2,
        )
        .await
        .unwrap();
    UnknownSetup {
        claimed,
        fence,
        snapshot_object_id,
        transcript_ref,
        now,
    }
}

async fn replay_unknown_finish(store: &SqliteStore, setup: &UnknownSetup) {
    store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: setup.fence.clone(),
                outcome: ModelCallEffectOutcome::OutcomeUnknown {
                    error_bytes: unknown_error(),
                },
                checkpoint: checkpoint(
                    &setup.claimed,
                    &setup.snapshot_object_id,
                    &setup.transcript_ref,
                    LlmLogicalCallStatus::OutcomeUnknown,
                    None,
                ),
            },
            setup.now + 3,
        )
        .await
        .unwrap();
}

fn resolution_command(
    setup: &UnknownSetup,
    resolution_id: &str,
    key: &str,
    kind: EffectResolutionKind,
    decision: serde_json::Value,
    result_object_id: Option<String>,
    evidence_object_id: Option<String>,
) -> ResolveEffectUnknownCommand {
    ResolveEffectUnknownCommand {
        resolution_id: resolution_id.into(),
        effect_id: "effect-1".into(),
        expected_effect_attempt_id: "effect-attempt-1".into(),
        expected_run_control_epoch: setup.claimed.run_control_epoch,
        command_idempotency_key: key.into(),
        kind,
        decision,
        result_object_id,
        evidence_object_id,
        actor_kind: EffectResolutionActorKind::Human,
        actor_id: Some("operator-1".into()),
    }
}

async fn assert_projection(
    store: &SqliteStore,
    effect: &str,
    model: &str,
    attempt: &str,
    wait: &str,
    blocker: &str,
    instance: &str,
) {
    let row = store.db.query_one(sql(
        "SELECT e.status AS effect_status, mc.status AS model_status, ea.status AS attempt_status, w.status AS wait_status, wb.status AS blocker_status, ni.status AS instance_status FROM effects e JOIN model_calls mc ON mc.id = e.model_call_id JOIN effect_attempts ea ON ea.effect_id = e.id JOIN wait_blockers wb ON wb.blocker_kind = 'effect' AND wb.blocker_id = e.id JOIN node_waits w ON w.id = wb.wait_id JOIN node_instances ni ON ni.id = e.node_instance_id WHERE e.id = 'effect-1'",
        vec![],
    )).await.unwrap().unwrap();
    assert_eq!(row.try_get::<String>("", "effect_status").unwrap(), effect);
    assert_eq!(row.try_get::<String>("", "model_status").unwrap(), model);
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        attempt
    );
    assert_eq!(row.try_get::<String>("", "wait_status").unwrap(), wait);
    assert_eq!(
        row.try_get::<String>("", "blocker_status").unwrap(),
        blocker
    );
    assert_eq!(
        row.try_get::<String>("", "instance_status").unwrap(),
        instance
    );
}

async fn assert_resume_attempt(store: &SqliteStore, node_instance_id: &str) {
    let row = store.db.query_one(sql(
        "SELECT invocation_kind, status FROM node_attempts WHERE node_instance_id = ? AND attempt_no = 2",
        vec![node_instance_id.into()],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "invocation_kind").unwrap(),
        "resume"
    );
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "queued");
}

async fn load_checkpoint(store: &SqliteStore, node_instance_id: &str) -> LlmLoopCheckpoint {
    let row = store
        .db
        .query_one(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    load_object_json(
        &store.db,
        &row.try_get::<String>("", "checkpoint_object_id").unwrap(),
    )
    .await
    .unwrap()
}

fn unknown_error() -> Vec<u8> {
    canonical::to_vec(&json!({"code":"provider_result_unobservable"})).unwrap()
}
