use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    runtime::{RunContextCommand, StartRunCommand},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{StorageError, graph::helpers::sql};

use super::{applied_graph, store};

#[tokio::test]
async fn context_patch_commits_projection_and_replays_idempotently() {
    let store = store().await;
    let run = run(&store, "context-basic").await;
    let command = command(
        &run.context_id,
        &run.branch_id,
        &run.input_commit_id,
        "set-scene",
        vec![JsonPatchOp::Add {
            path: "/scene".into(),
            value: json!({"name":"hall"}),
        }],
    );
    let first = store.commit_context_patch(command.clone()).await.unwrap();
    let replay = store.commit_context_patch(command).await.unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.parent_commit_ids, [run.input_commit_id]);
    let context = store
        .get_working_context(&run.context_id, &run.branch_id)
        .await
        .unwrap();
    assert_eq!(context.head_commit_id, first.id);
    assert_eq!(context.value, json!({"scene":{"name":"hall"}}));
    assert_eq!(count(&store, "domain_events").await, 1);
    assert_eq!(count(&store, "version_commits").await, 2);
}

#[tokio::test]
async fn stale_disjoint_patch_rebases_but_overlap_conflicts() {
    let store = store().await;
    let run = run(&store, "context-rebase").await;
    let base = run.input_commit_id.clone();
    let scene = store
        .commit_context_patch(command(
            &run.context_id,
            &run.branch_id,
            &base,
            "scene",
            vec![JsonPatchOp::Add {
                path: "/scene".into(),
                value: json!({"name":"hall"}),
            }],
        ))
        .await
        .unwrap();
    let flags = store
        .commit_context_patch(command(
            &run.context_id,
            &run.branch_id,
            &base,
            "flags",
            vec![JsonPatchOp::Add {
                path: "/flags".into(),
                value: json!({"ready":true}),
            }],
        ))
        .await
        .unwrap();
    assert_eq!(flags.parent_commit_ids, [scene.id]);
    assert_eq!(
        store
            .get_working_context(&run.context_id, &run.branch_id)
            .await
            .unwrap()
            .value,
        json!({"scene":{"name":"hall"},"flags":{"ready":true}})
    );

    let conflict = store
        .commit_context_patch(command(
            &run.context_id,
            &run.branch_id,
            &base,
            "rename-scene",
            vec![JsonPatchOp::Replace {
                path: "/scene/name".into(),
                value: json!("garden"),
            }],
        ))
        .await
        .unwrap_err();
    assert!(matches!(conflict, StorageError::Conflict("state_conflict")));
}

#[tokio::test]
async fn concurrent_append_is_ordered_and_element_id_is_deduplicated() {
    let store = store().await;
    let run = run(&store, "context-append").await;
    let initialized = store
        .commit_context_patch(command(
            &run.context_id,
            &run.branch_id,
            &run.input_commit_id,
            "init-messages",
            vec![JsonPatchOp::Add {
                path: "/messages".into(),
                value: json!([]),
            }],
        ))
        .await
        .unwrap();
    for (operation, element, text) in [
        ("append-a", "message-a", "a"),
        ("append-b", "message-b", "b"),
        ("append-a-duplicate", "message-a", "ignored"),
    ] {
        store
            .commit_context_patch(command(
                &run.context_id,
                &run.branch_id,
                &initialized.id,
                operation,
                vec![JsonPatchOp::Append {
                    path: "/messages".into(),
                    element_id: element.into(),
                    value: json!({"id":element,"text":text}),
                }],
            ))
            .await
            .unwrap();
    }
    assert_eq!(
        store
            .get_working_context(&run.context_id, &run.branch_id)
            .await
            .unwrap()
            .value["messages"],
        json!([
            {"id":"message-a","text":"a"},
            {"id":"message-b","text":"b"}
        ])
    );
}

#[tokio::test]
async fn operation_id_conflict_does_not_mutate_context() {
    let store = store().await;
    let run = run(&store, "context-idempotency").await;
    let first = command(
        &run.context_id,
        &run.branch_id,
        &run.input_commit_id,
        "same-operation",
        vec![JsonPatchOp::Add {
            path: "/value".into(),
            value: json!(1),
        }],
    );
    store.commit_context_patch(first).await.unwrap();
    let conflict = store
        .commit_context_patch(command(
            &run.context_id,
            &run.branch_id,
            &run.input_commit_id,
            "same-operation",
            vec![JsonPatchOp::Add {
                path: "/value".into(),
                value: json!(2),
            }],
        ))
        .await
        .unwrap_err();
    assert!(matches!(conflict, StorageError::IdempotencyConflict));
    assert_eq!(
        store
            .get_working_context(&run.context_id, &run.branch_id)
            .await
            .unwrap()
            .value,
        json!({"value":1})
    );
}

fn command(
    context: &str,
    branch: &str,
    base: &str,
    operation: &str,
    ops: Vec<JsonPatchOp>,
) -> CommitContextPatchCommand {
    CommitContextPatchCommand {
        patch: StatePatch {
            aggregate_kind: AggregateKind::WorkingContext,
            aggregate_id: context.into(),
            lineage_key: branch.into(),
            base_commit_id: base.into(),
            operation_id: operation.into(),
            ops,
            schema_version: 1,
            policy_version: 1,
            author: ActorRef {
                kind: ActorKind::Application,
                id: Some("test".into()),
            },
        },
        origin_run_id: None,
        origin_node_instance_id: None,
    }
}

async fn run(store: &crate::SqliteStore, key: &str) -> zhuangsheng_core::runtime::RunView {
    let revision = applied_graph(store, key).await;
    store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: key.into(),
        })
        .await
        .unwrap()
}

async fn count(store: &crate::SqliteStore, table: &str) -> i64 {
    store
        .db
        .query_one_raw(sql(
            &format!("SELECT COUNT(*) AS count FROM {table}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
