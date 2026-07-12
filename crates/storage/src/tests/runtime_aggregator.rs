use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::graph::{ApplyGraphCommand, GraphRevisionView, UpdateGraphDraftCommand},
    graph::GraphDraft,
    runtime::{RunContextCommand, RunControlCommand, RunStatus, StartRunCommand},
    scheduler::Scheduler,
};

use crate::graph::helpers::{load_object_json, now_ms, sql};

#[tokio::test]
async fn aggregator_closes_tumbling_windows_by_count_then_timeout() {
    let (store, revision) = aggregator_graph("aggregate-count-timeout", 3, 2, 1_000).await;
    let run = start(&store, &revision, json!(["a", "b", "c"]), "count-timeout").await;
    let now = now_ms();
    let scheduler = Scheduler::new(store.clone(), "aggregator");
    scheduler.run_until_idle(now, 512).await.unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Waiting
    );
    let windows = window_rows(&store, &run).await;
    assert_eq!(
        windows,
        vec![
            ("completed".into(), Some("count".into()), 2),
            ("open".into(), None, 1)
        ]
    );

    scheduler.run_until_idle(now + 1_001, 512).await.unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Completed
    );
    let outputs = output_values(&store, &run).await;
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0]["closeReason"], "count");
    assert_eq!(outputs[0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(outputs[1]["closeReason"], "timeout");
    assert_eq!(outputs[1]["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn aggregator_count_one_closes_without_a_resume_attempt() {
    let (store, revision) = aggregator_graph("aggregate-one", 2, 1, 10_000).await;
    let run = start(&store, &revision, json!([1, 2]), "count-one").await;
    Scheduler::new(store.clone(), "aggregator-one")
        .run_until_idle(now_ms(), 512)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(output_values(&store, &run).await.len(), 2);
    let attempts = store.db.query_one_raw(sql(
        "SELECT COUNT(*) AS count FROM node_attempts WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ? AND node_id = 'aggregate')",
        vec![run.clone().into()],
    )).await.unwrap().unwrap().try_get::<i64>("", "count").unwrap();
    assert_eq!(attempts, 2);
}

#[tokio::test]
async fn aggregator_timeout_pauses_during_interrupt_and_resumes_durably() {
    let (store, revision) = aggregator_graph("aggregate-interrupt", 1, 2, 100).await;
    let run = start(&store, &revision, json!(["only"]), "interrupt").await;
    let now = now_ms();
    let scheduler = Scheduler::new(store.clone(), "aggregator-interrupt");
    scheduler.run_until_idle(now, 256).await.unwrap();
    store
        .request_interrupt(RunControlCommand {
            run_id: run.clone(),
            expected_epoch: 0,
            reason: None,
            idempotency_key: "interrupt-aggregate".into(),
        })
        .await
        .unwrap();
    assert_eq!(scheduler.run_until_idle(now + 101, 32).await.unwrap(), 0);
    assert_eq!(window_rows(&store, &run).await[0].0, "open");
    store
        .resume_interrupted(RunControlCommand {
            run_id: run.clone(),
            expected_epoch: 1,
            reason: None,
            idempotency_key: "resume-aggregate".into(),
        })
        .await
        .unwrap();
    scheduler.run_until_idle(now + 101, 256).await.unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        output_values(&store, &run).await[0]["closeReason"],
        "timeout"
    );
}

#[tokio::test]
async fn aggregator_window_is_cancelled_without_losing_its_audit_items() {
    let (store, revision) = aggregator_graph("aggregate-cancel", 1, 2, 10_000).await;
    let run = start(&store, &revision, json!(["only"]), "cancel").await;
    Scheduler::new(store.clone(), "aggregator-cancel")
        .run_until_idle(now_ms(), 256)
        .await
        .unwrap();
    store
        .request_cancel(RunControlCommand {
            run_id: run.clone(),
            expected_epoch: 0,
            reason: None,
            idempotency_key: "cancel-aggregate".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Cancelled
    );
    assert_eq!(window_rows(&store, &run).await[0].0, "cancelled");
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT c.coordinator_buffered_values, (SELECT COUNT(*) FROM aggregation_window_items i JOIN aggregation_windows w ON w.id = i.window_id WHERE w.run_id = c.run_id) AS audit_items FROM run_execution_counters c WHERE c.run_id = ?",
            vec![run.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<i64>("", "coordinator_buffered_values")
            .unwrap(),
        0
    );
    assert_eq!(row.try_get::<i64>("", "audit_items").unwrap(), 1);
}

async fn aggregator_graph(
    key: &str,
    input_count: usize,
    count: u64,
    timeout_ms: u64,
) -> (Arc<crate::SqliteStore>, GraphRevisionView) {
    let store = Arc::new(super::store().await);
    let graph = super::graph(&store, &format!("create-{key}")).await;
    let current = store.get_graph_draft(&graph.id).await.unwrap();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for index in 0..input_count {
        nodes.push(json!({"id":format!("input_{index:03}"),"kind":"input","runInputSelector":{"type":"json_pointer","pointer":format!("/items/{index}")}}));
    }
    if input_count > 1 {
        nodes.push(json!({"id":"merge","kind":"merge","mode":"any","inputs":(0..input_count).map(|index| json!({"name":format!("v{index:03}")})).collect::<Vec<_>>() }));
        for index in 0..input_count {
            edges.push(json!({"from":{"nodeId":format!("input_{index:03}"),"output":"default"},"to":{"nodeId":"merge","input":format!("v{index:03}")}}));
        }
        edges.push(json!({"from":{"nodeId":"merge","output":"default"},"to":{"nodeId":"aggregate","input":"default"}}));
    } else {
        edges.push(json!({"from":{"nodeId":"input_000","output":"default"},"to":{"nodeId":"aggregate","input":"default"}}));
    }
    nodes.push(json!({"id":"aggregate","kind":"aggregator","count":count,"timeoutMs":timeout_ms}));
    nodes.push(json!({"id":"output","kind":"output","outputKey":"items"}));
    edges.push(json!({"from":{"nodeId":"aggregate","output":"default"},"to":{"nodeId":"output","input":"default"}}));
    let document: GraphDraft = serde_json::from_value(json!({"graphId":graph.id,"nodes":nodes,"edges":edges,"outputContract":[{"key":"items","collection":"append","required":true}]})).unwrap();
    let draft = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: current.revision_token,
            document,
            idempotency_key: format!("draft-{key}"),
        })
        .await
        .unwrap();
    let revision = store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.id,
            expected_revision_token: draft.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap();
    (store, revision)
}

async fn start(
    store: &crate::SqliteStore,
    revision: &GraphRevisionView,
    items: Value,
    key: &str,
) -> String {
    store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id.clone(),
            input: json!({"items":items}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: format!("run-{key}"),
        })
        .await
        .unwrap()
        .id
}

async fn window_rows(store: &crate::SqliteStore, run: &str) -> Vec<(String, Option<String>, i64)> {
    store.db.query_all_raw(sql("SELECT status, close_reason, item_count FROM aggregation_windows WHERE run_id = ? ORDER BY opened_at, id", vec![run.into()])).await.unwrap().into_iter().map(|row| Ok((row.try_get("", "status")?, row.try_get("", "close_reason")?, row.try_get("", "item_count")?))).collect::<crate::StorageResult<_>>().unwrap()
}

async fn output_values(store: &crate::SqliteStore, run: &str) -> Vec<Value> {
    let rows = store
        .db
        .query_all_raw(sql(
            "SELECT value_object_id FROM run_output_values WHERE run_id = ? ORDER BY output_seq",
            vec![run.into()],
        ))
        .await
        .unwrap();
    let mut values = Vec::new();
    for row in rows {
        values.push(
            load_object_json(
                &store.db,
                &row.try_get::<String>("", "value_object_id").unwrap(),
            )
            .await
            .unwrap(),
        );
    }
    values
}
