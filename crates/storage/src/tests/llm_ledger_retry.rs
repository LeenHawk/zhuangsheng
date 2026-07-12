use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectAttemptFence, FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint,
        ModelCallEffectOutcome, PrepareModelCallRetryCommand, StartModelCallCommand,
    },
};

use crate::{
    StorageError,
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_ledger::{checkpoint, now_ms, prepare_command, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn retryable_unknown_keeps_fact_and_creates_new_fenced_attempt() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
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
    let prepared_checkpoint = checkpoint(
        &claimed,
        &snapshot_object_id,
        &transcript_ref,
        LlmLogicalCallStatus::Prepared,
        None,
    );
    store
        .prepare_model_call(
            prepare_command(&claimed, &snapshot, prepared_checkpoint.clone()),
            now,
        )
        .await
        .unwrap();
    let first_fence = EffectAttemptFence {
        invoking_node_attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
    };
    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: first_fence.clone(),
                provider_request_id: Some("provider-request-1".into()),
                checkpoint: transition_checkpoint(
                    &prepared_checkpoint,
                    &claimed.attempt_id,
                    "effect-attempt-1",
                    LlmLogicalCallStatus::Running,
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
                fence: first_fence,
                outcome: ModelCallEffectOutcome::RetryReady {
                    error_bytes: canonical::to_vec(&json!({"code":"transport_lost"})).unwrap(),
                },
                checkpoint: transition_checkpoint(
                    &prepared_checkpoint,
                    &claimed.attempt_id,
                    "effect-attempt-1",
                    LlmLogicalCallStatus::RetryReady,
                ),
                transcript: None,
            },
            now + 2,
        )
        .await
        .unwrap();

    let executor_object_id = store
        .db
        .query_one_raw(sql(
            "SELECT executor_object_id FROM node_attempts WHERE id = ?",
            vec![claimed.attempt_id.clone().into()],
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
            vec![(now + 3).into(), claimed.attempt_id.clone().into()],
        ))
        .await
        .unwrap();
    store
        .db
        .execute_raw(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, worker_id, lease_until, idempotency_key, executor_object_id, started_at) VALUES ('node-attempt-2', ?, 2, 0, 'reconcile', 'running', ?, 1, 'ledger-worker-2', ?, 'ledger-attempt-2', ?, ?)",
            vec![claimed.node_instance_id.clone().into(), i64::try_from(claimed.run_control_epoch).unwrap().into(), (now + 30_000).into(), executor_object_id.into(), (now + 3).into()],
        ))
        .await
        .unwrap();
    let retry_fence = EffectAttemptFence {
        invoking_node_attempt_id: "node-attempt-2".into(),
        worker_id: "ledger-worker-2".into(),
        lease_fence: 1,
        run_control_epoch: claimed.run_control_epoch,
    };
    let retry_checkpoint = transition_checkpoint(
        &prepared_checkpoint,
        "node-attempt-2",
        "effect-attempt-2",
        LlmLogicalCallStatus::Prepared,
    );
    let prepared = store
        .prepare_model_call_retry(
            PrepareModelCallRetryCommand {
                model_call_id: "model-call-1".into(),
                effect_attempt_id: "effect-attempt-2".into(),
                fence: retry_fence.clone(),
                checkpoint: retry_checkpoint.clone(),
            },
            now + 4,
        )
        .await
        .unwrap();
    assert!(!prepared.replayed);
    let replayed = store
        .prepare_model_call_retry(
            PrepareModelCallRetryCommand {
                model_call_id: "model-call-1".into(),
                effect_attempt_id: "effect-attempt-2".into(),
                fence: retry_fence.clone(),
                checkpoint: retry_checkpoint,
            },
            now + 5,
        )
        .await
        .unwrap();
    assert!(replayed.replayed);
    let conflicting_retry = store
        .prepare_model_call_retry(
            PrepareModelCallRetryCommand {
                model_call_id: "model-call-1".into(),
                effect_attempt_id: "different-effect-attempt".into(),
                fence: retry_fence,
                checkpoint: transition_checkpoint(
                    &prepared_checkpoint,
                    "node-attempt-2",
                    "different-effect-attempt",
                    LlmLogicalCallStatus::Prepared,
                ),
            },
            now + 6,
        )
        .await;
    assert!(matches!(
        conflicting_retry,
        Err(StorageError::Conflict("model_call_retry_replay"))
    ));
    let rows = store
        .db
        .query_all_raw(sql(
            "SELECT status FROM effect_attempts WHERE effect_id = 'effect-1' ORDER BY attempt_no",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].try_get::<String>("", "status").unwrap(),
        "outcome_unknown"
    );
    assert_eq!(rows[1].try_get::<String>("", "status").unwrap(), "prepared");
}

fn transition_checkpoint(
    checkpoint: &LlmLoopCheckpoint,
    updater_attempt_id: &str,
    effect_attempt_id: &str,
    status: LlmLogicalCallStatus,
) -> LlmLoopCheckpoint {
    let mut checkpoint = checkpoint.clone();
    checkpoint.last_updated_by_attempt_id = updater_attempt_id.into();
    checkpoint.effect_watermark = effect_attempt_id.into();
    let active = checkpoint.active_model_effect.as_mut().unwrap();
    active.status = status;
    active.response_ref = None;
    checkpoint.checksum.clear();
    checkpoint.seal().unwrap()
}
