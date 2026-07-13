use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    runtime::{ForkContextCommand, RunContextCommand, StartRunCommand},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use super::applied_graph;
use crate::{
    StorageError,
    graph::helpers::{now_ms, sql},
    tests::store,
};

const NOW: i64 = 1_700_000_600_000;

#[tokio::test]
async fn fork_reconstructs_reachable_history_and_replays_after_branch_advances() {
    let store = store().await;
    let revision = applied_graph(&store, "fork-context").await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"fork"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "fork-context-run".into(),
        })
        .await
        .unwrap();
    let advanced = store
        .commit_context_patch(patch(
            &run.context_id,
            &run.branch_id,
            &run.input_commit_id,
            "advance-source",
            "/marker",
        ))
        .await
        .unwrap();
    let command = ForkContextCommand {
        context_id: run.context_id.clone(),
        source_branch_id: run.branch_id.clone(),
        from_commit_id: run.input_commit_id.clone(),
        expected_source_head: Some(advanced.id.clone()),
        idempotency_key: "fork-history".into(),
    };
    let branch = store
        .fork_context_at(command.clone(), NOW + 1)
        .await
        .unwrap();
    let forked = store
        .get_working_context(&branch.context_id, &branch.branch_id)
        .await
        .unwrap();
    assert_eq!(forked.value, json!({}));
    store
        .commit_context_patch(patch(
            &branch.context_id,
            &branch.branch_id,
            &branch.head_commit_id,
            "advance-fork",
            "/forked",
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .fork_context_at(command.clone(), NOW + 2)
            .await
            .unwrap(),
        branch
    );
    store
        .maintain_content_objects(now_ms() + 60_001, 60_000, 1_000)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_context_at_commit(&run.input_commit_id)
            .await
            .unwrap()
            .value,
        json!({})
    );
    assert_eq!(
        store
            .get_working_context(&branch.context_id, &branch.branch_id)
            .await
            .unwrap()
            .value,
        json!({"forked":true})
    );
    let mut conflicting_replay = command;
    conflicting_replay.from_commit_id = advanced.id.clone();
    assert!(matches!(
        store.fork_context_at(conflicting_replay, NOW + 2).await,
        Err(StorageError::IdempotencyConflict)
    ));
    assert!(matches!(
        store
            .fork_context_at(
                ForkContextCommand {
                    context_id: run.context_id,
                    source_branch_id: run.branch_id,
                    from_commit_id: advanced.id,
                    expected_source_head: Some("commit_stale".into()),
                    idempotency_key: "fork-stale".into(),
                },
                NOW + 3,
            )
            .await,
        Err(StorageError::Conflict("context_head"))
    ));
    assert_eq!(branch_count(&store).await, 2);
}

#[tokio::test]
async fn fork_rejects_a_commit_from_another_context() {
    let store = store().await;
    let revision = applied_graph(&store, "fork-cross-context").await;
    let first = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id.clone(),
            input: json!({"message":"first"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "fork-cross-first".into(),
        })
        .await
        .unwrap();
    let second = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"second"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "fork-cross-second".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .fork_context_at(
                ForkContextCommand {
                    context_id: first.context_id,
                    source_branch_id: first.branch_id,
                    from_commit_id: second.input_commit_id,
                    expected_source_head: Some(first.input_commit_id),
                    idempotency_key: "fork-cross-context".into(),
                },
                NOW,
            )
            .await,
        Err(StorageError::Conflict("fork_commit_not_reachable"))
    ));
    assert_eq!(branch_count(&store).await, 2);
}

fn patch(
    context_id: &str,
    branch_id: &str,
    base_commit_id: &str,
    operation_id: &str,
    path: &str,
) -> CommitContextPatchCommand {
    CommitContextPatchCommand {
        patch: StatePatch {
            aggregate_kind: AggregateKind::WorkingContext,
            aggregate_id: context_id.into(),
            lineage_key: branch_id.into(),
            base_commit_id: base_commit_id.into(),
            operation_id: operation_id.into(),
            ops: vec![JsonPatchOp::Add {
                path: path.into(),
                value: json!(true),
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
    }
}

async fn branch_count(store: &crate::SqliteStore) -> i64 {
    store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM context_branches",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
