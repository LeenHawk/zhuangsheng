use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    graph::{
        FinalValueSelector, FinalValueSource, InputSelector, LlmMemoryBinding, NodeMemoryBinding,
        StaticContextWrite, StaticContextWriteOp, StaticContextWriteTiming,
    },
    scheduler::{BuiltinResult, FinalizeAttemptCommand},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{
    graph::helpers::sql,
    tests::{llm_tool_support::prepare_running_tool_attempt_with_memory, store},
};

#[tokio::test]
async fn static_write_commits_context_with_node_completion_and_replays_once() {
    let store = store().await;
    let claimed = prepare_running_tool_attempt_with_memory(&store, write_memory()).await;
    let command = completion(&claimed, "static-write-success");
    store
        .finalize_attempt(command.clone(), now())
        .await
        .unwrap();
    store.finalize_attempt(command, now() + 1).await.unwrap();

    let run = store.get_run(&claimed.run_id).await.unwrap();
    let context = store
        .get_working_context(&run.context_id, &run.branch_id)
        .await
        .unwrap();
    assert_eq!(context.value, json!({"scene":{"phase":"night"}}));
    let rows = store.db.query_all_raw(sql(
        "SELECT event_type FROM run_events WHERE run_id=? AND node_instance_id=? AND event_type IN ('state.patch.committed','node.completed') ORDER BY seq",
        vec![claimed.run_id.clone().into(),claimed.node_instance_id.clone().into()],
    )).await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.try_get::<String>("", "event_type").unwrap())
            .collect::<Vec<_>>(),
        vec!["state.patch.committed", "node.completed"]
    );
    let commits: i64 = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM node_output_commits WHERE node_instance_id=?",
            vec![claimed.node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(commits, 1);
}

#[tokio::test]
async fn overlapping_change_since_pinned_write_base_fails_without_overwrite() {
    let store = store().await;
    let claimed = prepare_running_tool_attempt_with_memory(&store, write_memory()).await;
    let run = store.get_run(&claimed.run_id).await.unwrap();
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: run.context_id.clone(),
                lineage_key: run.branch_id.clone(),
                base_commit_id: run.input_commit_id.clone(),
                operation_id: "concurrent-scene".into(),
                ops: vec![JsonPatchOp::Add {
                    path: "/scene".into(),
                    value: json!({"phase":"day"}),
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::Application,
                    id: Some("test".into()),
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap();

    store
        .finalize_attempt(completion(&claimed, "static-write-conflict"), now())
        .await
        .unwrap();
    let context = store
        .get_working_context(&run.context_id, &run.branch_id)
        .await
        .unwrap();
    assert_eq!(context.value, json!({"scene":{"phase":"day"}}));
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        zhuangsheng_core::runtime::RunStatus::Failed
    );
    let failed = store
        .db
        .query_one_raw(sql(
            "SELECT payload_json FROM run_events WHERE run_id=? AND event_type='node.failed'",
            vec![run.id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(&failed.try_get::<String>("", "payload_json").unwrap()).unwrap();
    assert_eq!(payload["code"], "state_conflict");
}

fn write_memory() -> LlmMemoryBinding {
    LlmMemoryBinding {
        node: NodeMemoryBinding {
            reads: vec![],
            working_writes: vec![StaticContextWrite {
                id: "save-scene".into(),
                timing: StaticContextWriteTiming::AfterNodeCompleted,
                target_scope: "run-context".into(),
                path: "/scene".into(),
                op: StaticContextWriteOp::Add,
                value_from: Some(FinalValueSelector {
                    source: FinalValueSource::Output,
                    source_name: "default".into(),
                    selector: InputSelector::WholeValue,
                }),
            }],
        },
        tools: vec![],
    }
}

fn completion(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    key: &str,
) -> FinalizeAttemptCommand {
    FinalizeAttemptCommand {
        wakeup_id: claimed.wakeup_id.clone(),
        attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
        result_idempotency_key: key.into(),
        result: BuiltinResult::Completed {
            outputs: BTreeMap::from([("default".into(), json!({"phase":"night"}))]),
        },
    }
}

fn now() -> i64 {
    super::llm_ledger::now_ms() + 2
}
