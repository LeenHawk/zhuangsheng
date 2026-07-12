use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectAttemptFence, LlmLogicalCallStatus, PersistLlmStreamChunkCommand,
        StartModelCallCommand, ir::LlmStreamEventIr,
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
async fn stream_chunk_is_fenced_bounded_and_idempotent() {
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
    let prepared = checkpoint(
        &claimed,
        &snapshot_object_id,
        &transcript_ref,
        LlmLogicalCallStatus::Prepared,
        None,
    );
    store
        .prepare_model_call(prepare_command(&claimed, &snapshot, prepared), now)
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
                provider_request_id: Some("stream-request-1".into()),
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
    let first = store
        .persist_llm_stream_chunk(
            command(&claimed.node_instance_id, &fence, started()),
            now + 2,
        )
        .await
        .unwrap();
    assert!(!first.replayed);
    let replay = store
        .persist_llm_stream_chunk(
            command(&claimed.node_instance_id, &fence, started()),
            now + 3,
        )
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.durable_seq, first.durable_seq);
    let conflict = store
        .persist_llm_stream_chunk(
            command(
                &claimed.node_instance_id,
                &fence,
                vec![LlmStreamEventIr::ReasoningDelta {
                    call_id: "model-call-1".into(),
                    seq: 0,
                    item_id: "reasoning-1".into(),
                    text: "different".into(),
                }],
            ),
            now + 4,
        )
        .await;
    assert!(matches!(
        conflict,
        Err(StorageError::Conflict("llm_stream_chunk_replay"))
    ));
    let count = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM run_events WHERE event_type = 'llm.stream.chunk'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(count, 1);
}

fn command(
    node_instance_id: &str,
    fence: &EffectAttemptFence,
    events: Vec<LlmStreamEventIr>,
) -> PersistLlmStreamChunkCommand {
    PersistLlmStreamChunkCommand {
        node_instance_id: node_instance_id.into(),
        model_call_id: "model-call-1".into(),
        effect_attempt_id: "effect-attempt-1".into(),
        chunk_no: 1,
        fence: fence.clone(),
        events,
    }
}

fn started() -> Vec<LlmStreamEventIr> {
    vec![LlmStreamEventIr::Started {
        call_id: "model-call-1".into(),
        seq: 0,
    }]
}
