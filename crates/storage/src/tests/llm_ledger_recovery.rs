use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{LlmLogicalCallStatus, LlmLoopCheckpoint},
};

use crate::{
    graph::helpers::{load_object_json, put_inline_object, sql},
    tests::{
        llm_ledger::{checkpoint, now_ms, prepare_command, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn expired_lease_supersedes_prepared_effect_before_reconcile() {
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
    store
        .db
        .execute_raw(sql(
            "UPDATE node_attempts SET lease_until = ? WHERE id = ?",
            vec![(now - 1).into(), claimed.attempt_id.clone().into()],
        ))
        .await
        .unwrap();

    assert_eq!(store.recover_expired_leases(now).await.unwrap(), 1);

    let ledger = store
        .db
        .query_one_raw(sql(
            "SELECT ea.status AS attempt_status, e.status AS effect_status, mc.status AS model_status FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id JOIN model_calls mc ON mc.id = e.model_call_id WHERE ea.id = 'effect-attempt-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ledger.try_get::<String>("", "attempt_status").unwrap(),
        "superseded_before_start"
    );
    assert_eq!(
        ledger.try_get::<String>("", "effect_status").unwrap(),
        "pending"
    );
    assert_eq!(
        ledger.try_get::<String>("", "model_status").unwrap(),
        "retry_ready"
    );
    let checkpoint_object_id = store
        .db
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "checkpoint_object_id")
        .unwrap();
    let recovered: LlmLoopCheckpoint = load_object_json(&store.db, &checkpoint_object_id)
        .await
        .unwrap();
    assert!(recovered.checksum_is_valid());
    assert_eq!(
        recovered.active_model_effect.unwrap().status,
        LlmLogicalCallStatus::RetryReady
    );
    let replacement = store
        .db
        .query_one_raw(sql(
            "SELECT invocation_kind, status FROM node_attempts WHERE node_instance_id = ? AND attempt_no = 2",
            vec![claimed.node_instance_id.into()],
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
