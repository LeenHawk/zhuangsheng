use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::graph::{ApplyGraphCommand, GraphRevisionView, UpdateGraphDraftCommand},
    graph::{DraftNodeKind, GraphDraft, OutputPortDefinition},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler, SchedulerWork},
};

use crate::graph::helpers::{load_object_json, now_ms, sql};

#[tokio::test]
async fn expand_emits_index_then_edge_order_atomically() {
    let (store, revision) = expand_graph("expand-order", 8, 100, true, None).await;
    let run = start(&store, &revision, json!(["a", "b", "c"]), "order").await;
    Scheduler::new(store.clone(), "expand-order")
        .run_until_idle(now_ms(), 512)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Completed
    );
    for key in ["left", "right"] {
        let values = output_values(&store, &run, key).await;
        assert_eq!(
            values
                .iter()
                .map(|value| value["index"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            values
                .iter()
                .map(|value| value["item"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }
    let rows = expanded_queue_rows(&store, &run).await;
    assert_eq!(rows.len(), 6);
    let edge_order: Vec<_> = revision
        .definition
        .edges
        .iter()
        .filter(|edge| edge.from.node_id == "expand")
        .map(|edge| edge.id.clone())
        .collect();
    let mut edge_order = edge_order;
    edge_order.sort();
    for (position, (sequence, edge, emission)) in rows.iter().enumerate() {
        assert_eq!(*emission as usize, position / 2);
        assert_eq!(edge, &edge_order[position % 2]);
        if position > 0 {
            assert!(sequence > &rows[position - 1].0);
        }
    }
    let events = store.list_run_events(&run, 0, 1000).await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "coordination.expand_completed"
                && event.payload["itemCount"] == 3)
    );
}

#[tokio::test]
async fn expand_empty_array_completes_without_emission() {
    let (store, revision) = expand_graph("expand-empty", 8, 100, false, None).await;
    let run = start(&store, &revision, json!([]), "empty").await;
    Scheduler::new(store.clone(), "expand-empty")
        .run_until_idle(now_ms(), 256)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run).await.unwrap().status,
        RunStatus::Completed
    );
    assert!(expanded_queue_rows(&store, &run).await.is_empty());
    assert!(output_values(&store, &run, "left").await.is_empty());
}

#[tokio::test]
async fn expand_item_and_queue_limits_fail_with_zero_batch_emission() {
    let (store, revision) = expand_graph("expand-item-limit", 2, 100, true, None).await;
    let run = start(&store, &revision, json!([1, 2, 3]), "item-limit").await;
    Scheduler::new(store.clone(), "expand-item-limit")
        .run_until_idle(now_ms(), 256)
        .await
        .unwrap();
    assert_eq!(store.get_run(&run).await.unwrap().status, RunStatus::Failed);
    assert!(expanded_queue_rows(&store, &run).await.is_empty());

    let (store, revision) = expand_graph("expand-queue-limit", 3, 5, true, None).await;
    let run = start(&store, &revision, json!([1, 2, 3]), "queue-limit").await;
    Scheduler::new(store.clone(), "expand-queue-limit")
        .run_until_idle(now_ms(), 256)
        .await
        .unwrap();
    assert_eq!(store.get_run(&run).await.unwrap().status, RunStatus::Failed);
    assert!(expanded_queue_rows(&store, &run).await.is_empty());
}

#[tokio::test]
async fn expand_schema_failure_rolls_back_every_emission() {
    let envelope_schema = super::schema(json!({
        "type":"object",
        "properties":{"index":{"type":"integer"},"item":{"type":"integer"}},
        "required":["index","item"],
        "additionalProperties":false
    }));
    let (store, revision) =
        expand_graph("expand-schema", 4, 100, true, Some(envelope_schema)).await;
    let run = start(&store, &revision, json!([1, "bad"]), "schema").await;
    Scheduler::new(store.clone(), "expand-schema")
        .run_until_idle(now_ms(), 256)
        .await
        .unwrap();
    assert_eq!(store.get_run(&run).await.unwrap().status, RunStatus::Failed);
    assert!(expanded_queue_rows(&store, &run).await.is_empty());
}

#[tokio::test]
async fn expand_finalize_replay_does_not_duplicate_the_batch() {
    let (store, revision) = expand_graph("expand-replay", 4, 100, true, None).await;
    let run = start(&store, &revision, json!(["a", "b"]), "replay").await;
    let now = now_ms();
    let input = match store
        .claim_next_work("expand-replay", now, now + 30_000)
        .await
        .unwrap()
        .unwrap()
    {
        SchedulerWork::Attempt(attempt)
            if matches!(&attempt.node.kind, DraftNodeKind::Input { .. }) =>
        {
            attempt
        }
        other => panic!("unexpected work before input: {other:?}"),
    };
    store.mark_attempt_running(&input, now).await.unwrap();
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: input.wakeup_id.clone(),
                attempt_id: input.attempt_id.clone(),
                worker_id: input.worker_id.clone(),
                lease_fence: input.lease_fence,
                run_control_epoch: input.run_control_epoch,
                result_idempotency_key: "expand-replay-input".into(),
                result: BuiltinResult::Completed {
                    outputs: input.inputs.clone(),
                },
            },
            now,
        )
        .await
        .unwrap();
    let expand = loop {
        match store
            .claim_next_work("expand-replay", now, now + 30_000)
            .await
            .unwrap()
            .unwrap()
        {
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => store
                .activate_if_ready(&wakeup_id, &run_id, &node_id, now)
                .await
                .unwrap(),
            SchedulerWork::Settle { wakeup_id, run_id } => {
                store.settle_run(&wakeup_id, &run_id, now).await.unwrap()
            }
            SchedulerWork::Attempt(attempt)
                if matches!(attempt.node.kind, DraftNodeKind::Expand { .. }) =>
            {
                break attempt;
            }
            SchedulerWork::Noop => {}
            other => panic!("unexpected work before expand: {other:?}"),
        }
    };
    store.mark_attempt_running(&expand, now).await.unwrap();
    let command = FinalizeAttemptCommand {
        wakeup_id: expand.wakeup_id.clone(),
        attempt_id: expand.attempt_id.clone(),
        worker_id: expand.worker_id.clone(),
        lease_fence: expand.lease_fence,
        run_control_epoch: expand.run_control_epoch,
        result_idempotency_key: "expand-replay-result".into(),
        result: BuiltinResult::Expanded {
            output: "default".into(),
            values: vec![json!({"index":0,"item":"a"}), json!({"index":1,"item":"b"})],
        },
    };
    store.finalize_attempt(command.clone(), now).await.unwrap();
    store.finalize_attempt(command, now).await.unwrap();
    assert_eq!(expanded_queue_rows(&store, &run).await.len(), 4);
    let events = store.list_run_events(&run, 0, 1000).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "coordination.expand_completed")
            .count(),
        1
    );
}

async fn expand_graph(
    key: &str,
    max_items: u64,
    max_pending: u64,
    required: bool,
    output_schema: Option<zhuangsheng_core::schema::JsonSchemaSpec>,
) -> (Arc<crate::SqliteStore>, GraphRevisionView) {
    let store = Arc::new(super::store().await);
    let graph = super::graph(&store, &format!("create-{key}")).await;
    let current = store.get_graph_draft(&graph.id).await.unwrap();
    let mut document: GraphDraft = serde_json::from_value(json!({
        "graphId":graph.id,
        "nodes":[
            {"id":"input","kind":"input"},
            {"id":"expand","kind":"expand","maxItems":max_items},
            {"id":"left_output","kind":"output","outputKey":"left"},
            {"id":"right_output","kind":"output","outputKey":"right"}
        ],
        "edges":[
            {"from":{"nodeId":"input","output":"default"},"to":{"nodeId":"expand","input":"default"}},
            {"from":{"nodeId":"expand","output":"default"},"to":{"nodeId":"left_output","input":"default"}},
            {"from":{"nodeId":"expand","output":"default"},"to":{"nodeId":"right_output","input":"default"}}
        ],
        "outputContract":[
            {"key":"left","collection":"append","required":required},
            {"key":"right","collection":"append","required":required}
        ],
        "limits":{"maxPendingQueueValues":max_pending}
    })).unwrap();
    if let Some(schema) = output_schema {
        let expand = document
            .nodes
            .iter_mut()
            .find(|node| matches!(node.kind, DraftNodeKind::Expand { .. }))
            .unwrap();
        expand.outputs = vec![OutputPortDefinition {
            name: "default".into(),
            schema: Some(schema),
        }];
    }
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
    input: Value,
    key: &str,
) -> String {
    store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id.clone(),
            input,
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: format!("run-{key}"),
        })
        .await
        .unwrap()
        .id
}

async fn output_values(store: &crate::SqliteStore, run: &str, key: &str) -> Vec<Value> {
    let rows = store.db.query_all_raw(sql("SELECT value_object_id FROM run_output_values WHERE run_id = ? AND output_key = ? ORDER BY output_seq", vec![run.into(), key.into()])).await.unwrap();
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

async fn expanded_queue_rows(store: &crate::SqliteStore, run: &str) -> Vec<(i64, String, i64)> {
    store.db.query_all_raw(sql(
        "SELECT q.enqueue_seq, q.edge_id, q.producer_emission_index FROM edge_queue_values q JOIN node_instances ni ON ni.id = q.producer_instance_id WHERE q.run_id = ? AND ni.node_id = 'expand' ORDER BY q.enqueue_seq",
        vec![run.into()],
    )).await.unwrap().into_iter().map(|row| Ok((row.try_get("", "enqueue_seq")?, row.try_get("", "edge_id")?, row.try_get("", "producer_emission_index")?))).collect::<crate::StorageResult<_>>().unwrap()
}
