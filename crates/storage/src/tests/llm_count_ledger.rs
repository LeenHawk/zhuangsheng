use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        ActiveCountEffectCheckpoint, CountCallOutcome, CountExecutionPin, CountResultSource,
        EffectAttemptFence, EffectRetryPolicy, FinishCountCallCommand, LlmLogicalCallStatus,
        LlmLoopCheckpoint, PrepareCountCallCommand, PrepareCountCallRetryCommand,
        StartCountCallCommand,
    },
    scheduler::ClaimedAttempt,
};

use crate::{
    SqliteStore, StorageError,
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_graph::count_operation,
        llm_ledger::{now_ms, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn local_count_is_one_logical_budget_item_and_replays_exactly() {
    let store = store().await;
    let setup = setup(&store, false).await;
    let prepared = store
        .prepare_count_call(
            prepare_command(&setup, prepared_checkpoint(&setup)),
            setup.now,
        )
        .await
        .unwrap();
    assert!(!prepared.replayed);
    let replayed = store
        .prepare_count_call(
            prepare_command(&setup, prepared_checkpoint(&setup)),
            setup.now + 1,
        )
        .await
        .unwrap();
    assert!(replayed.replayed);
    assert_eq!(replayed.trim_candidate_ref, prepared.trim_candidate_ref);
    let completed = count_checkpoint(
        &setup,
        &prepared.trim_candidate_ref,
        "count-effect-attempt-1",
        &setup.claimed.attempt_id,
        LlmLogicalCallStatus::Completed,
        Some(CountResultSource::Local),
        None,
    );
    store
        .finish_count_call(
            FinishCountCallCommand {
                effect_attempt_id: "count-effect-attempt-1".into(),
                fence: setup.fence.clone(),
                outcome: CountCallOutcome::Completed {
                    token_count: 321,
                    source: CountResultSource::Local,
                },
                checkpoint: completed.clone(),
            },
            setup.now + 2,
        )
        .await
        .unwrap();
    store
        .finish_count_call(
            FinishCountCallCommand {
                effect_attempt_id: "count-effect-attempt-1".into(),
                fence: setup.fence.clone(),
                outcome: CountCallOutcome::Completed {
                    token_count: 321,
                    source: CountResultSource::Local,
                },
                checkpoint: completed.clone(),
            },
            setup.now + 3,
        )
        .await
        .unwrap();
    let conflict = store
        .finish_count_call(
            FinishCountCallCommand {
                effect_attempt_id: "count-effect-attempt-1".into(),
                fence: setup.fence.clone(),
                outcome: CountCallOutcome::Completed {
                    token_count: 322,
                    source: CountResultSource::Local,
                },
                checkpoint: completed,
            },
            setup.now + 4,
        )
        .await;
    assert!(matches!(
        conflict,
        Err(StorageError::Conflict("count_call_finish_replay"))
    ));
    let row = store.db.query_one_raw(sql(
        "SELECT cc.status, cc.result_source, cp.checkpoint_object_id, (SELECT COUNT(*) FROM count_calls WHERE node_instance_id = cc.node_instance_id) AS logical_count FROM count_calls cc JOIN llm_loop_checkpoints cp ON cp.node_instance_id = cc.node_instance_id WHERE cc.id = 'count-call-1'",
        vec![],
    )).await.unwrap().unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "completed");
    assert_eq!(row.try_get::<String>("", "result_source").unwrap(), "local");
    assert_eq!(row.try_get::<i64>("", "logical_count").unwrap(), 1);
    let checkpoint: LlmLoopCheckpoint = crate::graph::helpers::load_object_json(
        &store.db,
        &row.try_get::<String>("", "checkpoint_object_id").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(checkpoint.count_calls_used, 1);
    assert_eq!(
        checkpoint.active_count_effect.unwrap().result_source,
        Some(CountResultSource::Local)
    );
    assert_eq!(count_events(&store).await, 2);
}

#[tokio::test]
async fn provider_unknown_retries_same_candidate_then_falls_back_local() {
    let store = store().await;
    let setup = setup(&store, true).await;
    let prepared = store
        .prepare_count_call(
            prepare_command(&setup, prepared_checkpoint(&setup)),
            setup.now,
        )
        .await
        .unwrap();
    let running = count_checkpoint(
        &setup,
        &prepared.trim_candidate_ref,
        "count-effect-attempt-1",
        &setup.claimed.attempt_id,
        LlmLogicalCallStatus::Running,
        None,
        None,
    );
    store
        .start_count_call(
            StartCountCallCommand {
                effect_attempt_id: "count-effect-attempt-1".into(),
                fence: setup.fence.clone(),
                provider_request_id: Some("count-provider-request-1".into()),
                checkpoint: running.clone(),
            },
            setup.now + 1,
        )
        .await
        .unwrap();
    store
        .start_count_call(
            StartCountCallCommand {
                effect_attempt_id: "count-effect-attempt-1".into(),
                fence: setup.fence.clone(),
                provider_request_id: Some("count-provider-request-1".into()),
                checkpoint: running,
            },
            setup.now + 2,
        )
        .await
        .unwrap();
    let retry_ready = count_checkpoint(
        &setup,
        &prepared.trim_candidate_ref,
        "count-effect-attempt-1",
        &setup.claimed.attempt_id,
        LlmLogicalCallStatus::RetryReady,
        None,
        None,
    );
    store
        .finish_count_call(
            FinishCountCallCommand {
                effect_attempt_id: "count-effect-attempt-1".into(),
                fence: setup.fence.clone(),
                outcome: CountCallOutcome::RetryReady {
                    error_bytes: canonical::to_vec(&json!({"code":"count_transport_lost"}))
                        .unwrap(),
                },
                checkpoint: retry_ready,
            },
            setup.now + 3,
        )
        .await
        .unwrap();

    let retry_fence = install_reconcile_attempt(&store, &setup, setup.now + 4).await;
    let retry_checkpoint = count_checkpoint(
        &setup,
        &prepared.trim_candidate_ref,
        "count-effect-attempt-2",
        "count-reconcile-attempt",
        LlmLogicalCallStatus::Prepared,
        None,
        None,
    );
    let retried = store
        .prepare_count_call_retry(
            PrepareCountCallRetryCommand {
                count_call_id: "count-call-1".into(),
                effect_attempt_id: "count-effect-attempt-2".into(),
                fence: retry_fence.clone(),
                checkpoint: retry_checkpoint.clone(),
            },
            setup.now + 5,
        )
        .await
        .unwrap();
    assert_eq!(retried.trim_candidate_ref, prepared.trim_candidate_ref);
    let replayed = store
        .prepare_count_call_retry(
            PrepareCountCallRetryCommand {
                count_call_id: "count-call-1".into(),
                effect_attempt_id: "count-effect-attempt-2".into(),
                fence: retry_fence.clone(),
                checkpoint: retry_checkpoint,
            },
            setup.now + 6,
        )
        .await
        .unwrap();
    assert!(replayed.replayed);
    let completed = count_checkpoint(
        &setup,
        &prepared.trim_candidate_ref,
        "count-effect-attempt-2",
        "count-reconcile-attempt",
        LlmLogicalCallStatus::Completed,
        Some(CountResultSource::Local),
        None,
    );
    store
        .finish_count_call(
            FinishCountCallCommand {
                effect_attempt_id: "count-effect-attempt-2".into(),
                fence: retry_fence,
                outcome: CountCallOutcome::Completed {
                    token_count: 333,
                    source: CountResultSource::Local,
                },
                checkpoint: completed,
            },
            setup.now + 7,
        )
        .await
        .unwrap();
    let rows = store.db.query_all_raw(sql(
        "SELECT status FROM effect_attempts WHERE effect_id = 'count-effect-1' ORDER BY attempt_no",
        vec![],
    )).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].try_get::<String>("", "status").unwrap(),
        "outcome_unknown"
    );
    assert_eq!(
        rows[1].try_get::<String>("", "status").unwrap(),
        "succeeded"
    );
    let logical_count: i64 = store
        .db
        .query_one_raw(sql("SELECT COUNT(*) AS count FROM count_calls", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(logical_count, 1);
    assert_eq!(count_events(&store).await, 5);
}

struct CountSetup {
    claimed: ClaimedAttempt,
    fence: EffectAttemptFence,
    snapshot_object_id: String,
    transcript_ref: String,
    pin: CountExecutionPin,
    candidate_bytes: Vec<u8>,
    request_bytes: Vec<u8>,
    now: i64,
}

async fn setup(store: &SqliteStore, provider: bool) -> CountSetup {
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
    let max_input = snapshot.limits.max_input_tokens.unwrap();
    let pin = CountExecutionPin {
        generation_operation: snapshot.operation,
        provider_count_operation_key: provider.then_some(count_operation()),
        local_counter_id: "gproxy_tokenize".into(),
        local_counter_version: 1,
        fallback_policy_version: 1,
        safety_margin_tokens: max_input.div_ceil(20).max(256),
    };
    let fence = EffectAttemptFence {
        invoking_node_attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
    };
    CountSetup {
        claimed,
        fence,
        snapshot_object_id,
        transcript_ref,
        pin,
        candidate_bytes: canonical::to_vec(&json!({"messages":[{"role":"user","text":"hello"}]}))
            .unwrap(),
        request_bytes: canonical::to_vec(&json!({"model":"roleplay-model","input":"hello"}))
            .unwrap(),
        now,
    }
}

fn prepare_command(setup: &CountSetup, checkpoint: LlmLoopCheckpoint) -> PrepareCountCallCommand {
    PrepareCountCallCommand {
        count_call_id: "count-call-1".into(),
        effect_id: "count-effect-1".into(),
        effect_attempt_id: "count-effect-attempt-1".into(),
        node_instance_id: setup.claimed.node_instance_id.clone(),
        originating_attempt_id: setup.claimed.attempt_id.clone(),
        count_ordinal: 1,
        channel_id: setup
            .claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .channel
            .channel_id
            .clone(),
        pin: setup.pin.clone(),
        trim_candidate_bytes: setup.candidate_bytes.clone(),
        request_bytes: setup.request_bytes.clone(),
        effect_idempotency_key: format!("count-effect:{}:1", setup.claimed.node_instance_id),
        retry_policy: EffectRetryPolicy {
            max_attempts: 2,
            backoff_ms: vec![50],
        },
        checkpoint,
        candidate_transcript: None,
    }
}

fn prepared_checkpoint(setup: &CountSetup) -> LlmLoopCheckpoint {
    count_checkpoint(
        setup,
        "",
        "count-effect-attempt-1",
        &setup.claimed.attempt_id,
        LlmLogicalCallStatus::Prepared,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn count_checkpoint(
    setup: &CountSetup,
    candidate_ref: &str,
    effect_attempt_id: &str,
    updater_attempt_id: &str,
    status: LlmLogicalCallStatus,
    result_source: Option<CountResultSource>,
    result_ref: Option<String>,
) -> LlmLoopCheckpoint {
    let candidate_digest = canonical::hash_bytes(&setup.candidate_bytes);
    let request_digest = canonical::hash_bytes(&setup.request_bytes);
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: setup.claimed.node_instance_id.clone(),
        last_updated_by_attempt_id: updater_attempt_id.into(),
        graph_revision_id: setup
            .claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .graph_revision_id
            .clone(),
        registry_snapshot: setup
            .claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .tool_registry
            .clone(),
        context_snapshot_ref: setup.snapshot_object_id.clone(),
        read_set_digest: canonical::hash(&json!({})).unwrap(),
        model_call_no: 0,
        transcript_ref: setup.transcript_ref.clone(),
        continuation_ref: None,
        active_model_effect: None,
        active_count_effect: Some(ActiveCountEffectCheckpoint {
            count_call_id: "count-call-1".into(),
            effect_id: "count-effect-1".into(),
            count_ordinal: 1,
            count_execution_pin_digest: setup.pin.digest().unwrap(),
            trim_candidate_ref: candidate_ref.into(),
            trim_candidate_digest: candidate_digest,
            request_digest,
            status,
            result_source,
            result_ref,
        }),
        current_batch: vec![],
        model_calls_used: 0,
        count_calls_used: 1,
        tool_calls_used: 0,
        effect_watermark: effect_attempt_id.into(),
        wait_ids: vec![],
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}

async fn install_reconcile_attempt(
    store: &SqliteStore,
    setup: &CountSetup,
    now: i64,
) -> EffectAttemptFence {
    let executor = store
        .db
        .query_one_raw(sql(
            "SELECT executor_object_id FROM node_attempts WHERE id = ?",
            vec![setup.claimed.attempt_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "executor_object_id")
        .unwrap();
    store
        .db
        .execute_raw(sql(
            "UPDATE node_attempts SET status = 'completed', finished_at = ? WHERE id = ?",
            vec![now.into(), setup.claimed.attempt_id.clone().into()],
        ))
        .await
        .unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, worker_id, lease_until, idempotency_key, executor_object_id, started_at) VALUES ('count-reconcile-attempt', ?, 2, 0, 'reconcile', 'running', ?, 1, 'count-reconcile-worker', ?, 'count-reconcile-key', ?, ?)",
        vec![setup.claimed.node_instance_id.clone().into(), i64::try_from(setup.claimed.run_control_epoch).unwrap().into(), (now + 30_000).into(), executor.into(), now.into()],
    )).await.unwrap();
    EffectAttemptFence {
        invoking_node_attempt_id: "count-reconcile-attempt".into(),
        worker_id: "count-reconcile-worker".into(),
        lease_fence: 1,
        run_control_epoch: setup.claimed.run_control_epoch,
    }
}

async fn count_events(store: &SqliteStore) -> i64 {
    store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM run_events WHERE event_type LIKE 'llm.count.%'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
