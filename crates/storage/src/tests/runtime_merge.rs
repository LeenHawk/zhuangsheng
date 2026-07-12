use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::graph::{ApplyGraphCommand, GraphRevisionView, UpdateGraphDraftCommand},
    graph::{DraftNodeKind, GraphDraft, InputSelector},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler, SchedulerWork},
};

use crate::graph::helpers::{load_object_json, now_ms, sql};

use super::store;

#[tokio::test]
async fn merge_any_selects_global_earliest_and_rechecks_remaining_input() {
    let store = Arc::new(store().await);
    let revision = merge_graph(&store, "merge-any", false).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"left":"L","right":"R"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "merge-any-run".into(),
        })
        .await
        .unwrap();
    let now = now_ms();
    let mut deferred = Vec::new();
    let mut input_attempts = 0;
    while input_attempts < 2 {
        match store
            .claim_next_work("merge-stage", now, now + 30_000)
            .await
            .unwrap()
            .unwrap()
        {
            SchedulerWork::Attempt(attempt)
                if matches!(&attempt.node.kind, DraftNodeKind::Input { .. }) =>
            {
                store.mark_attempt_running(&attempt, now).await.unwrap();
                store
                    .finalize_attempt(
                        FinalizeAttemptCommand {
                            wakeup_id: attempt.wakeup_id.clone(),
                            attempt_id: attempt.attempt_id.clone(),
                            worker_id: attempt.worker_id.clone(),
                            lease_fence: attempt.lease_fence,
                            run_control_epoch: attempt.run_control_epoch,
                            result_idempotency_key: format!("stage:{}", attempt.attempt_id),
                            result: BuiltinResult::Completed {
                                outputs: attempt.inputs.clone(),
                            },
                        },
                        now,
                    )
                    .await
                    .unwrap();
                input_attempts += 1;
            }
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => deferred.push((wakeup_id, run_id, node_id)),
            SchedulerWork::Settle { wakeup_id, run_id } => {
                store.settle_run(&wakeup_id, &run_id, now).await.unwrap();
            }
            SchedulerWork::Noop => {}
            other => panic!("unexpected staged work: {other:?}"),
        }
    }
    assert_eq!(pending_merge_values(&store, &run.id).await.len(), 2);
    for (wakeup, run_id, node_id) in deferred {
        store
            .activate_if_ready(&wakeup, &run_id, &node_id, now)
            .await
            .unwrap();
    }
    Scheduler::new(store.clone(), "merge-finish")
        .run_until_idle(now, 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        output_values(&store, &run.id).await,
        vec![json!("L"), json!("R")]
    );
    let selections = selection_payloads(&store, &run.id).await;
    assert_eq!(selections.len(), 2);
    assert_eq!(selections[0]["selectedPort"], "left");
    assert!(
        selections[0]["enqueueSeq"].as_i64().unwrap()
            < selections[1]["enqueueSeq"].as_i64().unwrap()
    );
}

#[tokio::test]
async fn merge_selector_failure_is_durable_and_does_not_wedge_the_wakeup() {
    let store = Arc::new(store().await);
    let revision = merge_graph(&store, "merge-invalid", true).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"left":"L","right":"R"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "merge-invalid-run".into(),
        })
        .await
        .unwrap();
    let scheduler = Scheduler::new(store.clone(), "merge-invalid");
    scheduler.run_until_idle(now_ms(), 128).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Failed
    );
    assert_eq!(scheduler.run_until_idle(now_ms(), 8).await.unwrap(), 0);
    let row = store.db.query_one_raw(sql(
        "SELECT ni.status AS instance_status, a.status AS attempt_status FROM node_instances ni JOIN node_attempts a ON a.node_instance_id = ni.id WHERE ni.run_id = ? AND ni.node_id = 'merge'",
        vec![run.id.clone().into()],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "instance_status").unwrap(),
        "failed"
    );
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        "failed"
    );
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    assert!(events.iter().any(|event| {
        event.event_type == "node.failed" && event.payload["code"] == "input_contract_violation"
    }));
}

async fn pending_merge_values(store: &crate::SqliteStore, run_id: &str) -> Vec<i64> {
    store
        .db
        .query_all_raw(sql(
            "SELECT enqueue_seq FROM edge_queue_values WHERE run_id = ? AND consumed_at IS NULL ORDER BY enqueue_seq",
            vec![run_id.into()],
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "enqueue_seq").unwrap())
        .collect()
}

async fn output_values(store: &crate::SqliteStore, run_id: &str) -> Vec<Value> {
    let rows = store.db.query_all_raw(sql(
        "SELECT value_object_id FROM run_output_values WHERE run_id = ? AND output_key = 'items' ORDER BY output_seq",
        vec![run_id.into()],
    )).await.unwrap();
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

async fn selection_payloads(store: &crate::SqliteStore, run_id: &str) -> Vec<Value> {
    store.db.query_all_raw(sql(
        "SELECT payload_json FROM run_events WHERE run_id = ? AND event_type = 'coordination.merge_selected' ORDER BY seq",
        vec![run_id.into()],
    )).await.unwrap().iter().map(|row| {
        serde_json::from_str(&row.try_get::<String>("", "payload_json").unwrap()).unwrap()
    }).collect()
}

async fn merge_graph(
    store: &crate::SqliteStore,
    key: &str,
    invalid_left: bool,
) -> GraphRevisionView {
    let graph = super::graph(store, &format!("create-{key}")).await;
    let current = store.get_graph_draft(&graph.id).await.unwrap();
    let mut document: GraphDraft = serde_json::from_value(json!({
        "graphId":graph.id,
        "nodes":[
            {"id":"left","kind":"input","runInputSelector":{"type":"json_pointer","pointer":"/left"}},
            {"id":"right","kind":"input","runInputSelector":{"type":"json_pointer","pointer":"/right"}},
            {"id":"merge","kind":"merge","mode":"any","inputs":[{"name":"left"},{"name":"right"}]},
            {"id":"output","kind":"output","outputKey":"items"}
        ],
        "edges":[
            {"from":{"nodeId":"left","output":"default"},"to":{"nodeId":"merge","input":"left"}},
            {"from":{"nodeId":"right","output":"default"},"to":{"nodeId":"merge","input":"right"}},
            {"from":{"nodeId":"merge","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "outputContract":[{"key":"items","collection":"append","required":true}]
    })).unwrap();
    if invalid_left {
        let merge = document
            .nodes
            .iter_mut()
            .find(|node| node.id == "merge")
            .unwrap();
        merge.inputs[0].binding.selector = InputSelector::JsonPointer {
            pointer: "/missing".into(),
        };
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
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.id,
            expected_revision_token: draft.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap()
}
