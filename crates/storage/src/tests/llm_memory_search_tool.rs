use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{LlmLoopCheckpoint, MemorySearchToolEnvelope};

use crate::{
    StorageError,
    graph::helpers::{load_object_json, sql},
    tests::{
        llm_memory_search_support::{
            add_memory_record, prepare_memory_search_setup, search_batch_command,
        },
        store,
    },
};

#[tokio::test]
async fn memory_search_batch_pins_one_snapshot_persists_zero_results_and_replays() {
    let store = store().await;
    let setup = prepare_memory_search_setup(&store).await;
    let mut invalid = search_batch_command(&setup);
    invalid.calls[1].call_digest = "stale-digest".into();
    let error = store
        .execute_memory_search_tool_batch(invalid, setup.now + 3)
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::InvalidArgument(_)));
    assert_eq!(count(&store, "tool_calls").await, 0);

    let first = store
        .execute_memory_search_tool_batch(search_batch_command(&setup), setup.now + 4)
        .await
        .unwrap();
    assert!(!first.replayed);
    assert_eq!(first.calls.len(), 2);
    assert_eq!(
        first.calls[0].scope_snapshot_token,
        first.calls[1].scope_snapshot_token
    );
    assert_eq!(
        first.calls[0].scope_snapshot_token,
        "memory-scope:roleplay:revision:2"
    );
    assert_eq!(count(&store, "tool_call_bound_read_results").await, 2);
    assert_eq!(count(&store, "tool_call_read_set").await, 1);
    assert_eq!(
        count(&store, "effects WHERE tool_call_id IS NOT NULL").await,
        0
    );
    let first_envelope: MemorySearchToolEnvelope =
        load_object_json(&store.db, &first.calls[0].envelope_ref)
            .await
            .unwrap();
    let empty_envelope: MemorySearchToolEnvelope =
        load_object_json(&store.db, &first.calls[1].envelope_ref)
            .await
            .unwrap();
    assert_eq!(first_envelope.records.len(), 1);
    assert_eq!(
        first_envelope.records[0].summary,
        "Dragons guard the northern gate"
    );
    assert!(empty_envelope.records.is_empty());
    assert!(!empty_envelope.truncated);

    add_memory_record(
        &store,
        "late-dragon",
        "A second dragon guards the northern gate",
    )
    .await;
    let replay = store
        .execute_memory_search_tool_batch(search_batch_command(&setup), setup.now + 5)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.calls[0].envelope_ref, first.calls[0].envelope_ref);
    assert_eq!(
        replay.calls[0].scope_snapshot_token,
        "memory-scope:roleplay:revision:2"
    );
    assert_eq!(count(&store, "tool_calls").await, 2);
    assert_eq!(count(&store, "tool_call_read_set").await, 1);
    let checkpoint = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    assert!(
        checkpoint
            .current_batch
            .iter()
            .all(|call| call.output_ref.is_some())
    );
}

async fn count(store: &crate::SqliteStore, expression: &str) -> i64 {
    let statement = match expression {
        "tool_calls" => "SELECT COUNT(*) AS count FROM tool_calls",
        "tool_call_bound_read_results" => {
            "SELECT COUNT(*) AS count FROM tool_call_bound_read_results"
        }
        "tool_call_read_set" => "SELECT COUNT(*) AS count FROM tool_call_read_set",
        "effects WHERE tool_call_id IS NOT NULL" => {
            "SELECT COUNT(*) AS count FROM effects WHERE tool_call_id IS NOT NULL"
        }
        _ => panic!("unknown count expression"),
    };
    store
        .db
        .query_one(sql(statement, vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}

async fn load_checkpoint(store: &crate::SqliteStore, node_instance_id: &str) -> LlmLoopCheckpoint {
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
