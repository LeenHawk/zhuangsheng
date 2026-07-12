use std::{collections::BTreeMap, sync::Arc};

use sea_orm::ConnectionTrait;
use serde_json::Value;
use zhuangsheng_core::{
    application::memory::{
        ApplyMemoryProposalCommand, DecideMemoryProposalCommand, MemoryProposalDecision,
        ProposeMemoryChangeCommand,
    },
    graph::{
        MemoryQuery, MemoryReadConsistency, MemoryRecordStatus, RouterMemoryBinding,
        RouterReadBinding, RouterReadSource,
    },
    memory::{LongTermMemoryContentV1, MemoryProposalChangeInput, MemoryProposalStatus},
    router::evaluate_router,
    runtime::RunStatus,
    scheduler::{BuiltinResult, FinalizeAttemptCommand, Scheduler},
    state::{ActorKind, ActorRef},
};

use crate::graph::helpers::{now_ms, sql};

use super::{
    runtime_router_support::{
        applied_router_with_memory, claim_router_attempt, count, count_where, start,
    },
    store,
};

#[tokio::test]
async fn long_term_scope_phantom_triggers_router_reconcile() {
    let store = Arc::new(store().await);
    create_memory(
        &store,
        "dragon-memory",
        "A dragon sleeps in the western cave",
    )
    .await;
    let revision = applied_router_with_memory(
        &store,
        "router-long-memory",
        "size(memory.lore.records) == 1 && contains(lower_ascii(memory.lore.records[0].summary), \"phoenix\")",
        Some(RouterMemoryBinding {
            reads: vec![RouterReadBinding {
                id: "lore-read".into(),
                alias: "lore".into(),
                source: RouterReadSource::LongTermMemory {
                    scope: "roleplay".into(),
                    query: Some(MemoryQuery {
                        text: "phoenix".into(),
                        tags: vec!["lore".into()],
                        status: Some(MemoryRecordStatus::Active),
                    }),
                },
                required: false,
                consistency: MemoryReadConsistency::ValidateOnCommit,
                limit: Some(10),
                max_bytes: 8192,
            }],
        }),
    )
    .await;
    let run = store
        .start_run(start(&revision.id, "router-long-memory-run"))
        .await
        .unwrap();
    Scheduler::new(store.clone(), "long-memory-bootstrap")
        .run_one(now_ms())
        .await
        .unwrap();
    let now = now_ms();
    let claimed = claim_router_attempt(&store, now).await;
    store.mark_attempt_running(&claimed, now).await.unwrap();
    let old_memory = Value::Object(claimed.memory.clone().into_iter().collect());
    let old_error = evaluate_router(
        &claimed.node,
        &claimed.inputs,
        &old_memory,
        claimed.router_control.clone().unwrap(),
    )
    .unwrap_err();
    create_memory(&store, "phoenix-memory", "A phoenix returns at dawn").await;
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: claimed.wakeup_id,
                attempt_id: claimed.attempt_id.clone(),
                worker_id: claimed.worker_id,
                lease_fence: claimed.lease_fence,
                run_control_epoch: claimed.run_control_epoch,
                result_idempotency_key: format!("stale-long-memory:{}", claimed.attempt_id),
                result: BuiltinResult::RouterFailed { error: old_error },
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(count(&store, "router_decisions").await, 0);
    Scheduler::new(store.clone(), "long-memory-reconcile")
        .run_until_idle(now, 64)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(count(&store, "router_decisions").await, 1);
    assert_eq!(
        count_where(&store, "router_controls", "visits = 1").await,
        1
    );
    assert_eq!(
        count_where(
            &store,
            "node_read_set",
            "aggregate_kind = 'long_term_memory'"
        )
        .await,
        1
    );
    let tokens = store.db.query_all_raw(sql(
        "SELECT scope_snapshot_token FROM node_bound_read_results WHERE binding_id = 'lore-read' ORDER BY scope_snapshot_token",
        vec![],
    )).await.unwrap();
    let tokens: Vec<String> = tokens
        .iter()
        .map(|row| row.try_get("", "scope_snapshot_token").unwrap())
        .collect();
    assert_eq!(
        tokens,
        [
            "memory-scope:roleplay:revision:1",
            "memory-scope:roleplay:revision:2"
        ]
    );
}

async fn create_memory(store: &crate::SqliteStore, key: &str, text: &str) {
    let actor = ActorRef {
        kind: ActorKind::Application,
        id: Some("test".into()),
    };
    let proposal = store
        .propose_memory_change(ProposeMemoryChangeCommand {
            scope_id: "roleplay".into(),
            memory_id: None,
            expected_head_commit_id: None,
            change: MemoryProposalChangeInput::Create {
                content: LongTermMemoryContentV1 {
                    schema_version: 1,
                    text: text.into(),
                    tags: vec!["lore".into()],
                    attributes: BTreeMap::new(),
                },
            },
            reason: "test lore".into(),
            evidence_refs: vec![format!("evidence:{key}")],
            requested_by: actor.clone(),
            idempotency_key: format!("propose:{key}"),
            schema_version: 1,
            policy_version: 1,
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap();
    store
        .decide_memory_proposal(DecideMemoryProposalCommand {
            proposal_id: proposal.id.clone(),
            expected_status: MemoryProposalStatus::AwaitingReview,
            decision: MemoryProposalDecision::Approve,
            actor,
            idempotency_key: format!("approve:{key}"),
        })
        .await
        .unwrap();
    store
        .apply_memory_proposal(ApplyMemoryProposalCommand {
            proposal_id: proposal.id,
            expected_status: MemoryProposalStatus::Approved,
            idempotency_key: format!("apply:{key}"),
        })
        .await
        .unwrap();
}
