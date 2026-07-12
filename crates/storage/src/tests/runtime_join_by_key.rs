use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{runtime::RunStatus, scheduler::Scheduler};

use crate::graph::helpers::{load_object_json, now_ms, sql};

use super::runtime_join_support::{fixture, stage_join_inputs};

#[tokio::test]
async fn join_bypasses_incomplete_keys_and_strands_the_unmatched_item() {
    let fixture = fixture(
        "join-bypass",
        vec![item("a", "LA"), item("b", "LB")],
        vec![item("b", "RB")],
        8,
        8,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    run_activations(&fixture.store, activations, now).await;
    finish(&fixture, now).await;

    let outputs = output_values(&fixture).await;
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0]["key"], "b");
    assert_eq!(outputs[0]["values"]["left"]["value"], "LB");
    assert_eq!(outputs[0]["values"]["right"]["value"], "RB");
    assert_eq!(
        event_count(&fixture, "coordination.join_item_stranded").await,
        1
    );
}

#[tokio::test]
async fn join_preserves_fifo_within_each_port_and_key() {
    let fixture = fixture(
        "join-fifo",
        vec![item("same", "L1"), item("same", "L2")],
        vec![item("same", "R1"), item("same", "R2")],
        8,
        8,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    run_activations(&fixture.store, activations, now).await;
    finish(&fixture, now).await;
    let outputs = output_values(&fixture).await;
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0]["values"]["left"]["value"], "L1");
    assert_eq!(outputs[0]["values"]["right"]["value"], "R1");
    assert_eq!(outputs[1]["values"]["left"]["value"], "L2");
    assert_eq!(outputs[1]["values"]["right"]["value"], "R2");
}

#[tokio::test]
async fn join_uses_tuple_ready_sequence_and_matches_equal_numbers() {
    let decimal: Value = serde_json::from_str("1.0").unwrap();
    let fixture = fixture(
        "join-numeric",
        vec![json!({"id":1,"value":"L"})],
        vec![json!({"id":decimal,"value":"R"})],
        8,
        8,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    run_activations(&fixture.store, activations, now).await;
    finish(&fixture, now).await;
    assert_eq!(output_values(&fixture).await[0]["key"], 1);
    let payload = event_payloads(&fixture, "coordination.join_tuple_selected").await;
    let max_seq = payload[0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["enqueueSeq"].as_i64().unwrap())
        .max()
        .unwrap();
    assert_eq!(payload[0]["tupleReadySeq"], max_seq);
}

#[tokio::test]
async fn join_selects_the_ready_key_with_the_earliest_tuple_sequence() {
    let fixture = fixture(
        "join-ready-order",
        vec![item("a", "LA"), item("b", "LB")],
        vec![item("a", "RA"), item("b", "RB")],
        8,
        8,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    let expected = expected_first_key(&fixture).await;
    run_activations(&fixture.store, activations, now).await;
    finish(&fixture, now).await;
    let selections = event_payloads(&fixture, "coordination.join_tuple_selected").await;
    assert_eq!(selections.len(), 2);
    assert_eq!(selections[0]["key"], expected);
    let indexed = event_payloads(&fixture, "coordination.join_item_indexed").await;
    assert!(indexed.windows(2).all(|pair| {
        pair[0]["enqueueSeq"].as_i64().unwrap() < pair[1]["enqueueSeq"].as_i64().unwrap()
    }));
}

#[tokio::test]
async fn join_invalid_key_fails_durably_without_wedging() {
    let fixture = fixture(
        "join-invalid",
        vec![json!({"id":{"nested":true},"value":"L"})],
        vec![item("a", "R")],
        8,
        8,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    run_activations(&fixture.store, activations, now).await;
    assert_eq!(
        fixture.store.get_run(&fixture.run_id).await.unwrap().status,
        RunStatus::Failed
    );
    let failures = event_payloads(&fixture, "node.failed").await;
    assert_eq!(failures[0]["code"], "join_key_invalid");
    assert_eq!(
        Scheduler::new(fixture.store.clone(), "idle")
            .run_until_idle(now, 8)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn join_limits_fail_closed_without_dropping_the_excess_value() {
    let fixture = fixture(
        "join-limit",
        vec![item("a", "L1"), item("b", "L2")],
        vec![item("c", "R")],
        1,
        8,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    run_activations(&fixture.store, activations, now).await;
    assert_eq!(
        fixture.store.get_run(&fixture.run_id).await.unwrap().status,
        RunStatus::Failed
    );
    assert_eq!(event_count(&fixture, "run.failed").await, 1);
    let pending = fixture.store.db.query_one_raw(sql(
        "SELECT COUNT(*) AS count FROM edge_queue_values WHERE run_id = ? AND consumed_at IS NULL",
        vec![fixture.run_id.clone().into()],
    )).await.unwrap().unwrap().try_get::<i64>("", "count").unwrap();
    assert!(pending >= 2);
}

#[tokio::test]
async fn join_per_key_per_port_limit_is_enforced() {
    let fixture = fixture(
        "join-per-port-limit",
        vec![item("same", "L1"), item("same", "L2")],
        vec![item("other", "R")],
        8,
        1,
    )
    .await;
    let now = now_ms();
    let activations = stage_join_inputs(&fixture, now).await;
    run_activations(&fixture.store, activations, now).await;
    assert_eq!(
        fixture.store.get_run(&fixture.run_id).await.unwrap().status,
        RunStatus::Failed
    );
    let failures = event_payloads(&fixture, "run.failed").await;
    assert_eq!(failures[0]["code"], "run_limit_exceeded");
    assert!(
        failures[0]["safeMessage"]
            .as_str()
            .unwrap()
            .contains("per-key")
    );
}

#[tokio::test]
async fn join_index_is_idempotent_and_incomplete_keys_settle_as_stranded() {
    let fixture = fixture(
        "join-recovery",
        vec![item("a", "L")],
        vec![item("b", "R")],
        8,
        8,
    )
    .await;
    let now = now_ms();
    let mut activations = stage_join_inputs(&fixture, now).await;
    assert!(!activations.is_empty());
    activations.remove(0).run(&fixture.store, now).await;
    let first = buffer_count(&fixture).await;
    run_activations(&fixture.store, activations, now).await;
    assert_eq!(first, 2);
    assert_eq!(buffer_count(&fixture).await, 2);
    finish(&fixture, now).await;
    let statuses = fixture.store.db.query_one_raw(sql(
        "SELECT COUNT(*) AS count FROM coordination_buffer_items WHERE run_id = ? AND status = 'stranded'",
        vec![fixture.run_id.clone().into()],
    )).await.unwrap().unwrap().try_get::<i64>("", "count").unwrap();
    assert_eq!(statuses, 2);
}

fn item(id: &str, value: &str) -> Value {
    json!({"id":id,"value":value})
}

async fn run_activations(
    store: &crate::SqliteStore,
    activations: Vec<super::runtime_join_support::Activation>,
    now: i64,
) {
    for activation in activations {
        activation.run(store, now).await;
    }
}

async fn finish(fixture: &super::runtime_join_support::JoinFixture, now: i64) {
    Scheduler::new(fixture.store.clone(), "join-finish")
        .run_until_idle(now, 512)
        .await
        .unwrap();
    assert_eq!(
        fixture.store.get_run(&fixture.run_id).await.unwrap().status,
        RunStatus::Completed
    );
}

async fn output_values(fixture: &super::runtime_join_support::JoinFixture) -> Vec<Value> {
    let rows = fixture.store.db.query_all_raw(sql(
        "SELECT value_object_id FROM run_output_values WHERE run_id = ? AND output_key = 'items' ORDER BY output_seq",
        vec![fixture.run_id.clone().into()],
    )).await.unwrap();
    let mut result = Vec::new();
    for row in rows {
        result.push(
            load_object_json(
                &fixture.store.db,
                &row.try_get::<String>("", "value_object_id").unwrap(),
            )
            .await
            .unwrap(),
        );
    }
    result
}

async fn event_payloads(
    fixture: &super::runtime_join_support::JoinFixture,
    kind: &str,
) -> Vec<Value> {
    fixture
        .store
        .list_run_events(&fixture.run_id, 0, 1000)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.event_type == kind)
        .map(|event| event.payload)
        .collect()
}

async fn event_count(fixture: &super::runtime_join_support::JoinFixture, kind: &str) -> usize {
    event_payloads(fixture, kind).await.len()
}

async fn buffer_count(fixture: &super::runtime_join_support::JoinFixture) -> i64 {
    fixture
        .store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM coordination_buffer_items WHERE run_id = ?",
            vec![fixture.run_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}

async fn expected_first_key(fixture: &super::runtime_join_support::JoinFixture) -> Value {
    let ports: std::collections::HashMap<_, _> = fixture
        .revision
        .definition
        .edges
        .iter()
        .filter(|edge| edge.to.node_id == "join")
        .map(|edge| (edge.id.as_str(), edge.to.input.as_str()))
        .collect();
    let rows = fixture
        .store
        .db
        .query_all_raw(sql(
            "SELECT edge_id, enqueue_seq, value_object_id FROM edge_queue_values WHERE run_id = ? AND consumed_at IS NULL ORDER BY enqueue_seq",
            vec![fixture.run_id.clone().into()],
        ))
        .await
        .unwrap();
    let mut keys: std::collections::BTreeMap<String, std::collections::HashMap<String, i64>> =
        Default::default();
    for row in rows {
        let edge: String = row.try_get("", "edge_id").unwrap();
        let Some(port) = ports.get(edge.as_str()) else {
            continue;
        };
        let value: Value = load_object_json(
            &fixture.store.db,
            &row.try_get::<String>("", "value_object_id").unwrap(),
        )
        .await
        .unwrap();
        keys.entry(value["id"].as_str().unwrap().into())
            .or_default()
            .insert((*port).into(), row.try_get("", "enqueue_seq").unwrap());
    }
    let key = keys
        .into_iter()
        .filter(|(_, ports)| ports.len() == 2)
        .min_by_key(|(key, ports)| (*ports.values().max().unwrap(), key.clone()))
        .unwrap()
        .0;
    json!(key)
}
