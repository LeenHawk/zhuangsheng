use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    graph::EffectClassification,
    llm::{
        EffectRetryPolicy, PrepareInitialModelCallCommand,
        ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole},
    },
};

use crate::{
    StorageError,
    graph::helpers::sql,
    tests::{llm_ledger::prepare_running_llm_attempt, store},
};

#[tokio::test]
async fn initial_model_call_prepares_transcript_checkpoint_and_effect_atomically() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let read_set_digest = claimed
        .context_snapshot
        .as_ref()
        .unwrap()
        .read_set_digest
        .clone();
    let now = super::llm_ledger::now_ms();

    let wrong = store
        .prepare_initial_model_call(command(&claimed, &snapshot, "sha256:wrong"), now)
        .await;
    assert!(matches!(wrong, Err(StorageError::InvalidArgument(_))));
    assert_eq!(row_count(&store, "model_calls").await, 0);
    assert_eq!(row_count(&store, "effects").await, 0);
    assert_eq!(row_count(&store, "effect_attempts").await, 0);
    assert_eq!(row_count(&store, "llm_loop_checkpoints").await, 0);

    let prepared = store
        .prepare_initial_model_call(command(&claimed, &snapshot, &read_set_digest), now + 1)
        .await
        .unwrap();
    assert!(!prepared.prepared.replayed);
    assert!(prepared.checkpoint.checksum_is_valid());
    assert_eq!(prepared.checkpoint.model_call_no, 1);
    assert_eq!(prepared.checkpoint.model_calls_used, 1);
    assert_eq!(prepared.checkpoint.read_set_digest, read_set_digest);
    assert_eq!(row_count(&store, "model_calls").await, 1);
    assert_eq!(row_count(&store, "effects").await, 1);
    assert_eq!(row_count(&store, "effect_attempts").await, 1);
    assert_eq!(row_count(&store, "llm_loop_checkpoints").await, 1);

    let replay = store
        .prepare_initial_model_call(command(&claimed, &snapshot, &read_set_digest), now + 2)
        .await
        .unwrap();
    assert!(replay.prepared.replayed);
    assert_eq!(replay.checkpoint.checksum, prepared.checkpoint.checksum);
}

fn command(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    snapshot: &zhuangsheng_core::graph::LlmNodeExecutionSnapshot,
    read_set_digest: &str,
) -> PrepareInitialModelCallCommand {
    PrepareInitialModelCallCommand {
        model_call_id: "model-call-initial".into(),
        effect_id: "effect-initial".into(),
        effect_attempt_id: "effect-attempt-initial".into(),
        node_instance_id: claimed.node_instance_id.clone(),
        originating_attempt_id: claimed.attempt_id.clone(),
        channel_id: snapshot.channel.channel_id.clone(),
        operation: snapshot.operation.clone(),
        request_bytes: br#"{"model":"roleplay-model"}"#.to_vec(),
        transcript: vec![LlmTurnItemIr::Message {
            id: "message-user".into(),
            role: MessageRole::User,
            content: vec![LlmContentPartIr::Text {
                text: "hello".into(),
            }],
            provenance: None,
            placeholder: false,
        }],
        registry_snapshot: snapshot.tool_registry.clone(),
        read_set_digest: read_set_digest.into(),
        effect_kind: "model_generation".into(),
        effect_classification: EffectClassification::Idempotent,
        effect_operation_key: "llm.generate".into(),
        effect_idempotency_key: "model-call-initial".into(),
        retry_policy: EffectRetryPolicy {
            max_attempts: 2,
            backoff_ms: vec![100],
        },
    }
}

async fn row_count(store: &crate::SqliteStore, table: &str) -> i64 {
    let query = match table {
        "model_calls" => "SELECT COUNT(*) AS count FROM model_calls",
        "effects" => "SELECT COUNT(*) AS count FROM effects",
        "effect_attempts" => "SELECT COUNT(*) AS count FROM effect_attempts",
        "llm_loop_checkpoints" => "SELECT COUNT(*) AS count FROM llm_loop_checkpoints",
        _ => unreachable!(),
    };
    store
        .db
        .query_one(sql(query, Vec::new()))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
