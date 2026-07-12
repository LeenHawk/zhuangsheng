use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    graph::{MemoryReadConsistency, RouterMemoryBinding, RouterReadBinding, RouterReadSource},
    router::evaluate_router,
    runtime::{RunContextCommand, RunControlCommand, RunStatus, StartRunCommand},
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::graph::helpers::{load_object_json, now_ms, sql};

use super::{
    runtime_router_support::{
        applied_router, applied_router_with_memory, claim_router_attempt, commit_scene, count,
        count_where, start,
    },
    store,
};

#[tokio::test]
async fn router_decision_and_selected_emission_are_durable_and_atomic() {
    let store = Arc::new(store().await);
    let revision = applied_router(&store, "router-success", "inputs.default == \"hello\"").await;
    let run = store
        .start_run(start(&revision.id, "router-success"))
        .await
        .unwrap();
    Scheduler::new(store.clone(), "router-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        count_where(&store, "router_controls", "visits = 1").await,
        1
    );
    assert_eq!(count(&store, "router_activation_controls").await, 1);
    assert_eq!(
        count_where(&store, "router_decisions", "outcome = 'decision'").await,
        1
    );
    assert_eq!(
        count_where(&store, "run_events", "event_type = 'router.decision'").await,
        1
    );
    assert_eq!(count(&store, "edge_queue_values").await, 2);

    let row = store
        .db
        .query_one_raw(sql(
            "SELECT decision_object_id FROM router_decisions WHERE outcome = 'decision'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    let object_id: String = row.try_get("", "decision_object_id").unwrap();
    let decision: Value = load_object_json(&store.db, &object_id).await.unwrap();
    assert_eq!(decision["selectedPorts"], json!(["done"]));
    assert_eq!(decision["matchedRuleIds"], json!(["hello"]));
    assert!(decision.get("payload").is_none());
    assert!(decision["payloadRef"].as_str().is_some());
}

#[tokio::test]
async fn router_retry_reuses_activation_control_snapshot() {
    let store = Arc::new(store().await);
    let revision = applied_router(&store, "router-retry", "inputs.default == \"hello\"").await;
    let run = store
        .start_run(start(&revision.id, "router-retry"))
        .await
        .unwrap();
    let scheduler = Scheduler::new(store.clone(), "bootstrap-worker");
    assert!(scheduler.run_one(now_ms()).await.unwrap());

    let now = now_ms();
    let claimed = claim_router_attempt(&store, now).await;
    assert!(claimed.router_control.is_some());
    store.mark_attempt_running(&claimed, now).await.unwrap();
    Scheduler::new(store.clone(), "recovery-router")
        .run_until_idle(now + 2, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        count_where(&store, "router_controls", "visits = 1").await,
        1
    );
    assert_eq!(count(&store, "router_activation_controls").await, 1);
    assert_eq!(count(&store, "router_decisions").await, 1);
}

#[tokio::test]
async fn router_evaluation_error_records_error_without_emission() {
    let store = Arc::new(store().await);
    let revision = applied_router(&store, "router-error", "inputs.default.missing == true").await;
    let run = store
        .start_run(start(&revision.id, "router-error"))
        .await
        .unwrap();
    Scheduler::new(store.clone(), "router-error-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Failed
    );
    assert_eq!(
        count_where(&store, "router_decisions", "outcome = 'error'").await,
        1
    );
    assert_eq!(
        count_where(&store, "run_events", "event_type = 'router.decision_error'").await,
        1
    );
    assert_eq!(count(&store, "run_output_values").await, 0);
    assert_eq!(count(&store, "edge_queue_values").await, 1);
}

#[tokio::test]
async fn router_limit_uses_durable_candidate_visit_before_rules() {
    let store = Arc::new(store().await);
    let revision = applied_router(&store, "router-limit", "inputs.default.missing == true").await;
    let run = store
        .start_run(start(&revision.id, "router-limit"))
        .await
        .unwrap();
    let now = now_ms();
    Scheduler::new(store.clone(), "limit-bootstrap")
        .run_one(now)
        .await
        .unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO router_controls (run_id, node_id, visits, first_visited_at, updated_at) VALUES (?, 'router', 1, ?, ?)",
        vec![run.id.clone().into(), (now - 100).into(), now.into()],
    )).await.unwrap();
    Scheduler::new(store.clone(), "limit-worker")
        .run_until_idle(now + 1, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT visits, limit_reasons_json FROM router_activation_controls",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "visits").unwrap(), 2);
    assert_eq!(
        row.try_get::<String>("", "limit_reasons_json").unwrap(),
        r#"["max_visits"]"#
    );
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT decision_object_id FROM router_decisions",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    let decision: Value = load_object_json(
        &store.db,
        &row.try_get::<String>("", "decision_object_id").unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(decision["reason"], "limit");
    assert_eq!(decision["evaluatedRuleIds"], json!([]));
}

#[tokio::test]
async fn router_working_context_read_is_pinned_and_exposed_by_alias() {
    let store = Arc::new(store().await);
    let revision = applied_router_with_memory(
        &store,
        "router-memory",
        "memory.scene.found && memory.scene.value.phase == \"ending\"",
        Some(RouterMemoryBinding {
            reads: vec![RouterReadBinding {
                id: "scene-read".into(),
                alias: "scene".into(),
                source: RouterReadSource::WorkingContext {
                    scope: "story".into(),
                    path: "/scene".into(),
                },
                required: true,
                consistency: MemoryReadConsistency::Snapshot,
                limit: None,
                max_bytes: 4096,
            }],
        }),
    )
    .await;
    let seed = store
        .start_run(start(&revision.id, "router-memory-seed"))
        .await
        .unwrap();
    let commit = store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: seed.context_id.clone(),
                lineage_key: seed.branch_id.clone(),
                base_commit_id: seed.input_commit_id.clone(),
                operation_id: "seed-scene".into(),
                ops: vec![JsonPatchOp::Add {
                    path: "/scene".into(),
                    value: json!({"phase":"ending"}),
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
        .request_cancel(RunControlCommand {
            run_id: seed.id,
            expected_epoch: 0,
            idempotency_key: "cancel-memory-seed".into(),
            reason: None,
        })
        .await
        .unwrap();
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Existing {
                context_id: seed.context_id,
                branch_id: seed.branch_id,
                expected_head_commit_id: commit.id.clone(),
            },
            deadline_at: None,
            idempotency_key: "router-memory-run".into(),
        })
        .await
        .unwrap();
    Scheduler::new(store.clone(), "router-memory-bootstrap")
        .run_one(now_ms())
        .await
        .unwrap();
    let now = now_ms();
    let claimed = claim_router_attempt(&store, now).await;
    store.mark_attempt_running(&claimed, now).await.unwrap();
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: run.context_id.clone(),
                lineage_key: run.branch_id.clone(),
                base_commit_id: commit.id.clone(),
                operation_id: "advance-scene-after-read".into(),
                ops: vec![JsonPatchOp::Replace {
                    path: "/scene/phase".into(),
                    value: json!("middle"),
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
    Scheduler::new(store.clone(), "router-memory-recovery")
        .run_until_idle(now + 2, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    let rows = store
        .db
        .query_all_raw(sql(
            "SELECT commit_id, binding_id FROM node_read_set ORDER BY node_attempt_id",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row.try_get::<String>("", "commit_id").unwrap(), commit.id);
        assert_eq!(
            row.try_get::<String>("", "binding_id").unwrap(),
            "scene-read"
        );
    }
    assert_eq!(count(&store, "node_bound_read_results").await, 2);
}

#[tokio::test]
async fn validate_on_commit_reconciles_without_new_visit_or_input_consumption() {
    let store = Arc::new(store().await);
    let revision = applied_router_with_memory(
        &store,
        "router-reconcile",
        "memory.scene.value.phase == \"middle\"",
        Some(RouterMemoryBinding {
            reads: vec![RouterReadBinding {
                id: "scene-read".into(),
                alias: "scene".into(),
                source: RouterReadSource::WorkingContext {
                    scope: "story".into(),
                    path: "/scene".into(),
                },
                required: true,
                consistency: MemoryReadConsistency::ValidateOnCommit,
                limit: None,
                max_bytes: 4096,
            }],
        }),
    )
    .await;
    let seed = store
        .start_run(start(&revision.id, "router-reconcile-seed"))
        .await
        .unwrap();
    let first_commit = commit_scene(
        &store,
        &seed,
        &seed.input_commit_id,
        "ending",
        "seed-ending",
    )
    .await;
    store
        .request_cancel(RunControlCommand {
            run_id: seed.id,
            expected_epoch: 0,
            idempotency_key: "cancel-reconcile-seed".into(),
            reason: None,
        })
        .await
        .unwrap();
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Existing {
                context_id: seed.context_id,
                branch_id: seed.branch_id,
                expected_head_commit_id: first_commit.id.clone(),
            },
            deadline_at: None,
            idempotency_key: "router-reconcile-run".into(),
        })
        .await
        .unwrap();
    Scheduler::new(store.clone(), "reconcile-bootstrap")
        .run_one(now_ms())
        .await
        .unwrap();
    let now = now_ms();
    let claimed = claim_router_attempt(&store, now).await;
    store.mark_attempt_running(&claimed, now).await.unwrap();
    let memory = Value::Object(claimed.memory.clone().into_iter().collect());
    let old_result = evaluate_router(
        &claimed.node,
        &claimed.inputs,
        &memory,
        claimed.router_control.clone().unwrap(),
    )
    .unwrap_err();
    let second_commit =
        commit_scene(&store, &run, &first_commit.id, "middle", "advance-middle").await;
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: claimed.wakeup_id,
                attempt_id: claimed.attempt_id.clone(),
                worker_id: claimed.worker_id,
                lease_fence: claimed.lease_fence,
                run_control_epoch: claimed.run_control_epoch,
                result_idempotency_key: format!("stale-result:{}", claimed.attempt_id),
                result: BuiltinResult::RouterFailed { error: old_result },
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(count(&store, "router_decisions").await, 0);
    assert_eq!(
        count_where(&store, "router_controls", "visits = 1").await,
        1
    );
    assert_eq!(count(&store, "edge_queue_values").await, 1);
    Scheduler::new(store.clone(), "reconcile-worker")
        .run_until_idle(now, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(count(&store, "router_decisions").await, 1);
    assert_eq!(
        count_where(&store, "node_attempts", "invocation_kind = 'reconcile'").await,
        1
    );
    assert_eq!(
        count_where(&store, "router_controls", "visits = 1").await,
        1
    );
    let rows = store
        .db
        .query_all_raw(sql(
            "SELECT DISTINCT commit_id FROM node_read_set ORDER BY commit_id",
            vec![],
        ))
        .await
        .unwrap();
    let commits: Vec<String> = rows
        .iter()
        .map(|row| row.try_get("", "commit_id").unwrap())
        .collect();
    assert!(commits.contains(&first_commit.id));
    assert!(commits.contains(&second_commit.id));
}
