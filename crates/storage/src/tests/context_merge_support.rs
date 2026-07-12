use serde_json::{Value, json};
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    runtime::{ContextBranchView, ForkContextCommand, RunContextCommand, StartRunCommand},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use super::applied_graph;

pub(super) async fn temporary_root(
    store: &crate::SqliteStore,
    key: &str,
) -> (String, String, String) {
    let revision = applied_graph(store, &format!("merge-{key}")).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"merge"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: format!("merge-run-{key}"),
        })
        .await
        .unwrap();
    (run.context_id, run.branch_id, run.input_commit_id)
}

pub(super) async fn fork(
    store: &crate::SqliteStore,
    root: &(String, String, String),
    key: &str,
) -> ContextBranchView {
    store
        .fork_context_at(
            ForkContextCommand {
                context_id: root.0.clone(),
                source_branch_id: root.1.clone(),
                from_commit_id: root.2.clone(),
                expected_source_head: Some(root.2.clone()),
                idempotency_key: key.into(),
            },
            1_700_000_690_000,
        )
        .await
        .unwrap()
}

pub(super) async fn commit(
    store: &crate::SqliteStore,
    context: &str,
    branch: &str,
    base: &str,
    key: &str,
    value: i64,
) -> String {
    commit_value(store, context, branch, base, key, json!(value)).await
}

pub(super) async fn commit_value(
    store: &crate::SqliteStore,
    context: &str,
    branch: &str,
    base: &str,
    key: &str,
    value: Value,
) -> String {
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: context.into(),
                lineage_key: branch.into(),
                base_commit_id: base.into(),
                operation_id: format!("merge-test-{branch}-{key}"),
                ops: vec![JsonPatchOp::Add {
                    path: format!("/{key}"),
                    value,
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::User,
                    id: None,
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap()
        .id
}

pub(super) async fn append(
    store: &crate::SqliteStore,
    context: &str,
    branch: &str,
    base: &str,
    element_id: &str,
    value: Value,
) -> String {
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: context.into(),
                lineage_key: branch.into(),
                base_commit_id: base.into(),
                operation_id: format!("merge-append-{branch}-{element_id}"),
                ops: vec![JsonPatchOp::Append {
                    path: "/items".into(),
                    element_id: element_id.into(),
                    value,
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::User,
                    id: None,
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap()
        .id
}
