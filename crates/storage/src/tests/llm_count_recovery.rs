use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        ActiveCountEffectCheckpoint, CountExecutionPin, EffectAttemptFence, EffectRetryPolicy,
        LlmLogicalCallStatus, LlmLoopCheckpoint, PrepareCountCallCommand, StartCountCallCommand,
        ToolRegistrySnapshot,
    },
};

use crate::{
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_graph::count_operation,
        llm_ledger::{now_ms, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn expired_provider_count_reuses_logical_call_via_reconcile() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_ref = store
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
    let candidate =
        canonical::to_vec(&json!({"messages":[{"role":"user","text":"recover"}]})).unwrap();
    let request = canonical::to_vec(&json!({"model":"roleplay-model","input":"recover"})).unwrap();
    let max_input = snapshot.limits.max_input_tokens.unwrap();
    let pin = CountExecutionPin {
        generation_operation: snapshot.operation,
        provider_count_operation_key: Some(count_operation()),
        local_counter_id: "gproxy_tokenize".into(),
        local_counter_version: 1,
        fallback_policy_version: 1,
        safety_margin_tokens: max_input.div_ceil(20).max(256),
    };
    let prepared_checkpoint = checkpoint(
        &claimed,
        &snapshot_ref,
        &transcript_ref,
        &pin,
        &candidate,
        &request,
        "",
        LlmLogicalCallStatus::Prepared,
    );
    let prepared = store
        .prepare_count_call(
            PrepareCountCallCommand {
                count_call_id: "recovery-count-call".into(),
                effect_id: "recovery-count-effect".into(),
                effect_attempt_id: "recovery-count-attempt".into(),
                node_instance_id: claimed.node_instance_id.clone(),
                originating_attempt_id: claimed.attempt_id.clone(),
                count_ordinal: 1,
                channel_id: snapshot.channel.channel_id,
                pin: pin.clone(),
                trim_candidate_bytes: candidate.clone(),
                request_bytes: request.clone(),
                effect_idempotency_key: "recovery-count-effect-key".into(),
                retry_policy: EffectRetryPolicy {
                    max_attempts: 2,
                    backoff_ms: vec![10],
                },
                checkpoint: prepared_checkpoint,
            },
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
        .start_count_call(
            StartCountCallCommand {
                effect_attempt_id: "recovery-count-attempt".into(),
                fence,
                provider_request_id: Some("recovery-provider-request".into()),
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    &pin,
                    &candidate,
                    &request,
                    &prepared.trim_candidate_ref,
                    LlmLogicalCallStatus::Running,
                ),
            },
            now + 1,
        )
        .await
        .unwrap();
    store
        .db
        .execute(sql(
            "UPDATE node_attempts SET lease_until = ? WHERE id = ?",
            vec![now.into(), claimed.attempt_id.clone().into()],
        ))
        .await
        .unwrap();
    assert_eq!(store.recover_expired_leases(now + 2).await.unwrap(), 1);
    let row = store.db.query_one(sql(
        "SELECT cc.status AS count_status, ea.status AS attempt_status, (SELECT invocation_kind FROM node_attempts WHERE node_instance_id = cc.node_instance_id AND attempt_no = 2) AS replacement_kind FROM count_calls cc JOIN effects e ON e.count_call_id = cc.id JOIN effect_attempts ea ON ea.effect_id = e.id WHERE cc.id = 'recovery-count-call'",
        vec![],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "count_status").unwrap(),
        "retry_ready"
    );
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        "outcome_unknown"
    );
    assert_eq!(
        row.try_get::<String>("", "replacement_kind").unwrap(),
        "reconcile"
    );
}

#[allow(clippy::too_many_arguments)]
fn checkpoint(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    snapshot_ref: &str,
    transcript_ref: &str,
    pin: &CountExecutionPin,
    candidate: &[u8],
    request: &[u8],
    candidate_ref: &str,
    status: LlmLogicalCallStatus,
) -> LlmLoopCheckpoint {
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: claimed.node_instance_id.clone(),
        last_updated_by_attempt_id: claimed.attempt_id.clone(),
        graph_revision_id: claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .graph_revision_id
            .clone(),
        registry_snapshot: ToolRegistrySnapshot {
            revision: "empty-registry-v1".into(),
            entries: vec![],
        },
        context_snapshot_ref: snapshot_ref.into(),
        read_set_digest: canonical::hash(&json!({})).unwrap(),
        model_call_no: 0,
        transcript_ref: transcript_ref.into(),
        continuation_ref: None,
        active_model_effect: None,
        active_count_effect: Some(ActiveCountEffectCheckpoint {
            count_call_id: "recovery-count-call".into(),
            effect_id: "recovery-count-effect".into(),
            count_ordinal: 1,
            count_execution_pin_digest: pin.digest().unwrap(),
            trim_candidate_ref: candidate_ref.into(),
            trim_candidate_digest: canonical::hash_bytes(candidate),
            request_digest: canonical::hash_bytes(request),
            status,
            result_source: None,
            result_ref: None,
        }),
        current_batch: vec![],
        model_calls_used: 0,
        count_calls_used: 1,
        tool_calls_used: 0,
        effect_watermark: "recovery-count-attempt".into(),
        wait_ids: vec![],
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}
