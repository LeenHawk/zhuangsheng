use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::canonical;

use crate::{
    SqliteStore,
    graph::helpers::{new_id, put_inline_object, sql},
};

pub(super) async fn complete_with_reply(
    store: &SqliteStore,
    run_id: &str,
    output_commit: &str,
    now: i64,
) {
    let value = json!({
        "schemaVersion":1,"type":"assistant_reply",
        "content":[{"type":"text","text":"The archive remembers you."}]
    });
    let value_id = put_inline_object(&store.db, &canonical::to_vec(&value).unwrap(), now)
        .await
        .unwrap();
    let node = store
        .db
        .query_one_raw(sql(
            "SELECT id FROM node_instances WHERE run_id = ? ORDER BY id LIMIT 1",
            vec![run_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "id")
        .unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO run_output_values (id, run_id, output_key, collection_mode, output_seq, node_instance_id, value_object_id, created_at) VALUES (?, ?, 'reply', 'single', 1, ?, ?, ?)",
        vec![new_id("output").into(), run_id.into(), node.into(), value_id.clone().into(), now.into()],
    )).await.unwrap();
    let counter = next_event_seq(store, run_id).await;
    store.db.execute_raw(sql(
        "UPDATE graph_runs SET status = 'completed', output_commit_id = ?, run_outputs_object_id = ?, finished_at = ?, updated_at = ? WHERE id = ?",
        vec![output_commit.into(), value_id.clone().into(), now.into(), now.into(), run_id.into()],
    )).await.unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO run_events (id, run_id, seq, event_type, schema_version, importance, payload_json, created_at) VALUES (?, ?, ?, 'run.completed', 1, 'critical', ?, ?)",
        vec![new_id("event").into(), run_id.into(), counter.into(), json!({"schemaVersion":1,"outputCommitId":output_commit,"outputsRef":value_id}).to_string().into(), now.into()],
    )).await.unwrap();
    advance_event_seq(store, run_id).await;
}

pub(super) async fn terminalize_failed(store: &SqliteStore, run_id: &str, now: i64) {
    let counter = next_event_seq(store, run_id).await;
    store
        .db
        .execute_raw(sql(
            "UPDATE graph_runs SET status = 'failed', finished_at = ?, updated_at = ? WHERE id = ?",
            vec![now.into(), now.into(), run_id.into()],
        ))
        .await
        .unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO run_events (id, run_id, seq, event_type, schema_version, importance, payload_json, created_at) VALUES (?, ?, ?, 'run.failed', 1, 'critical', '{\"schemaVersion\":1}', ?)",
        vec![new_id("event").into(), run_id.into(), counter.into(), now.into()],
    )).await.unwrap();
    advance_event_seq(store, run_id).await;
}

async fn next_event_seq(store: &SqliteStore, run_id: &str) -> i64 {
    store
        .db
        .query_one_raw(sql(
            "SELECT next_seq FROM run_event_counters WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "next_seq")
        .unwrap()
}

async fn advance_event_seq(store: &SqliteStore, run_id: &str) {
    store
        .db
        .execute_raw(sql(
            "UPDATE run_event_counters SET next_seq = next_seq + 1 WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await
        .unwrap();
}
