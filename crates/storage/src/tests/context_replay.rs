use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::context::{CommitContextPatchCommand, CreateVersionSnapshotCommand},
    runtime::{RunContextCommand, StartRunCommand},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{StorageError, graph::helpers::sql};

use super::{applied_graph, store};

#[tokio::test]
async fn version_snapshot_accelerates_replay_and_preserves_append_deduplication() {
    let store = store().await;
    let run = run(&store, "context-replay").await;
    let initialized = commit(
        &store,
        &run,
        &run.input_commit_id,
        "init",
        vec![JsonPatchOp::Add {
            path: "/messages".into(),
            value: json!([]),
        }],
    )
    .await;
    let first = commit(
        &store,
        &run,
        &initialized,
        "append-a",
        vec![append("a", "first")],
    )
    .await;
    let snapshot = store
        .create_version_snapshot(CreateVersionSnapshotCommand {
            commit_id: first.clone(),
            retention_until: None,
            pinned: true,
        })
        .await
        .unwrap();
    assert_eq!(
        snapshot,
        store
            .create_version_snapshot(CreateVersionSnapshotCommand {
                commit_id: first,
                retention_until: None,
                pinned: true,
            })
            .await
            .unwrap()
    );
    let second = commit(
        &store,
        &run,
        &snapshot.commit_id,
        "append-b",
        vec![append("b", "second")],
    )
    .await;
    let head = commit(
        &store,
        &run,
        &second,
        "duplicate-a",
        vec![append("a", "ignored")],
    )
    .await;
    let replayed = store.get_context_at_commit(&head).await.unwrap();
    assert_eq!(
        replayed.value,
        json!({"messages":[
            {"id":"a","text":"first"},
            {"id":"b","text":"second"}
        ]})
    );
}

#[tokio::test]
async fn projection_can_be_rebuilt_from_authoritative_version_history() {
    let store = store().await;
    let run = run(&store, "context-rebuild").await;
    let head = commit(
        &store,
        &run,
        &run.input_commit_id,
        "set-scene",
        vec![JsonPatchOp::Add {
            path: "/scene".into(),
            value: json!({"phase":"ending"}),
        }],
    )
    .await;
    store
        .db
        .execute(sql(
            "UPDATE materialized_projections SET projection_json = '{\"corrupt\":true}' WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ?",
            vec![run.context_id.clone().into(), run.branch_id.clone().into()],
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_working_context(&run.context_id, &run.branch_id)
            .await
            .unwrap()
            .value,
        json!({"corrupt":true})
    );
    let rebuilt = store
        .rebuild_working_context_projection(&run.context_id, &run.branch_id, &head)
        .await
        .unwrap();
    assert_eq!(rebuilt.value, json!({"scene":{"phase":"ending"}}));
    assert_eq!(
        store
            .get_working_context(&run.context_id, &run.branch_id)
            .await
            .unwrap(),
        rebuilt
    );
}

#[tokio::test]
async fn snapshot_checksum_corruption_fails_closed() {
    let store = store().await;
    let run = run(&store, "context-snapshot-corrupt").await;
    store
        .create_version_snapshot(CreateVersionSnapshotCommand {
            commit_id: run.input_commit_id.clone(),
            retention_until: None,
            pinned: false,
        })
        .await
        .unwrap();
    store
        .db
        .execute(sql(
            "UPDATE version_snapshots SET checksum = 'sha256:broken' WHERE commit_id = ?",
            vec![run.input_commit_id.clone().into()],
        ))
        .await
        .unwrap();
    let error = store
        .get_context_at_commit(&run.input_commit_id)
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::Integrity(_)));
}

fn append(id: &str, text: &str) -> JsonPatchOp {
    JsonPatchOp::Append {
        path: "/messages".into(),
        element_id: id.into(),
        value: json!({"id":id,"text":text}),
    }
}

async fn commit(
    store: &crate::SqliteStore,
    run: &zhuangsheng_core::runtime::RunView,
    base: &str,
    operation: &str,
    ops: Vec<JsonPatchOp>,
) -> String {
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: run.context_id.clone(),
                lineage_key: run.branch_id.clone(),
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
        })
        .await
        .unwrap()
        .id
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
