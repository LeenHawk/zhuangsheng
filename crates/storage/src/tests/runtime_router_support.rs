use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        context::CommitContextPatchCommand,
        graph::{ApplyGraphCommand, UpdateGraphDraftCommand},
    },
    graph::{
        DraftGraphEdge, DraftGraphNode, DraftNodeKind, GraphInputRef, GraphOutputRef,
        OutputPortDefinition, RouterLimits, RouterMatchMode, RouterMemoryBinding, RouterRule,
    },
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::SchedulerWork,
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::graph::helpers::sql;

use super::{graph, run_draft};

pub(super) async fn applied_router(
    store: &crate::SqliteStore,
    key: &str,
    expression: &str,
) -> zhuangsheng_core::application::graph::GraphRevisionView {
    applied_router_with_memory(store, key, expression, None).await
}

pub(super) async fn applied_router_with_memory(
    store: &crate::SqliteStore,
    key: &str,
    expression: &str,
    memory: Option<RouterMemoryBinding>,
) -> zhuangsheng_core::application::graph::GraphRevisionView {
    let graph = graph(store, &format!("create-{key}")).await;
    let initial = store.get_graph_draft(&graph.id).await.unwrap();
    let mut draft = run_draft(&graph.id);
    draft.nodes.insert(1, router_node(expression, memory));
    draft.edges = vec![
        edge("input", "default", "router", "default"),
        edge("router", "done", "output", "default"),
    ];
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: initial.revision_token,
            document: draft,
            idempotency_key: format!("draft-{key}"),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap()
}

fn router_node(expression: &str, memory: Option<RouterMemoryBinding>) -> DraftGraphNode {
    DraftGraphNode {
        id: "router".into(),
        name: None,
        is_entry: None,
        inputs: vec![],
        outputs: vec![OutputPortDefinition {
            name: "done".into(),
            schema: None,
        }],
        timeout_ms: None,
        retry_policy: None,
        kind: DraftNodeKind::Router {
            dsl_version: "router-dsl-v1".into(),
            rules: vec![RouterRule {
                id: "hello".into(),
                when: expression.into(),
                outputs: vec!["done".into()],
            }],
            match_mode: RouterMatchMode::First,
            default_outputs: None,
            payload_port: Some("default".into()),
            memory,
            limits: Some(RouterLimits {
                max_visits_per_run: Some(1),
                timeout_ms_per_run: None,
                max_read_reconciles: None,
                on_limit_outputs: Some(vec!["done".into()]),
            }),
        },
    }
}

fn edge(from_node: &str, from: &str, to_node: &str, to: &str) -> DraftGraphEdge {
    DraftGraphEdge {
        id: None,
        from: GraphOutputRef {
            node_id: from_node.into(),
            output: from.into(),
        },
        to: GraphInputRef {
            node_id: to_node.into(),
            input: to.into(),
        },
    }
}

pub(super) fn start(revision_id: &str, key: &str) -> StartRunCommand {
    StartRunCommand {
        graph_revision_id: revision_id.into(),
        input: json!({"message":"hello"}),
        context: RunContextCommand::Temporary,
        deadline_at: None,
        idempotency_key: key.into(),
    }
}

pub(super) async fn count(store: &crate::SqliteStore, table: &str) -> i64 {
    count_where(store, table, "1 = 1").await
}

pub(super) async fn count_where(store: &crate::SqliteStore, table: &str, predicate: &str) -> i64 {
    store
        .db
        .query_one_raw(sql(
            &format!("SELECT COUNT(*) AS count FROM {table} WHERE {predicate}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}

pub(super) async fn claim_router_attempt(
    store: &crate::SqliteStore,
    now: i64,
) -> Box<zhuangsheng_core::scheduler::ClaimedAttempt> {
    for _ in 0..16 {
        match store
            .claim_next_work("dead-router", now, now + 1)
            .await
            .unwrap()
            .expect("pending Router work")
        {
            SchedulerWork::Attempt(attempt) => {
                assert_eq!(attempt.node.id, "router");
                return attempt;
            }
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
            SchedulerWork::Noop => {}
        }
    }
    panic!("Router attempt was not scheduled")
}

pub(super) async fn commit_scene(
    store: &crate::SqliteStore,
    run: &zhuangsheng_core::runtime::RunView,
    base_commit_id: &str,
    phase: &str,
    operation_id: &str,
) -> zhuangsheng_core::application::context::ContextCommitView {
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: run.context_id.clone(),
                lineage_key: run.branch_id.clone(),
                base_commit_id: base_commit_id.into(),
                operation_id: operation_id.into(),
                ops: vec![JsonPatchOp::Add {
                    path: "/scene".into(),
                    value: json!({"phase":phase}),
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
        .unwrap()
}
