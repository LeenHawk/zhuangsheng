use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::context_merge::{
    ExplicitMergeResolution, ExplicitMergeSelection, MergeContextCommand, MergeContextStatus,
    MergeSourceDisposition,
};

use crate::{graph::helpers::sql, tests::store};

use super::context_merge_support::{commit, fork, temporary_root};

#[tokio::test]
async fn clean_merge_creates_two_parent_commit_and_replays() {
    let store = store().await;
    let root = temporary_root(&store, "clean").await;
    let source = fork(&store, &root, "clean-source").await;
    let target = fork(&store, &root, "clean-target").await;
    let source_head = commit(
        &store,
        &root.0,
        &source.branch_id,
        &source.head_commit_id,
        "source",
        1,
    )
    .await;
    let target_head = commit(
        &store,
        &root.0,
        &target.branch_id,
        &target.head_commit_id,
        "target",
        2,
    )
    .await;
    let command = MergeContextCommand {
        context_id: root.0.clone(),
        source_branch_id: source.branch_id.clone(),
        target_branch_id: target.branch_id.clone(),
        expected_source_head: source_head.clone(),
        expected_target_head: target_head,
        source_disposition: MergeSourceDisposition::KeepActive,
        selections: vec![],
        idempotency_key: "clean-merge".into(),
    };
    let merged = store
        .merge_context_at(command.clone(), 1_700_000_700_000)
        .await
        .unwrap();
    assert_eq!(merged.status, MergeContextStatus::Merged);
    let commit_id = merged.merge_commit_id.clone().unwrap();
    let context = store
        .get_working_context(&root.0, &target.branch_id)
        .await
        .unwrap();
    assert_eq!(context.value, json!({"source":1,"target":2}));
    let parents = store
        .db
        .query_all_raw(sql(
            "SELECT parent_commit_id FROM commit_parents WHERE commit_id = ? ORDER BY parent_order",
            vec![commit_id.into()],
        ))
        .await
        .unwrap();
    assert_eq!(parents.len(), 2);
    assert_eq!(
        parents[0]
            .try_get::<String>("", "parent_commit_id")
            .unwrap(),
        command.expected_target_head
    );
    assert_eq!(
        parents[1]
            .try_get::<String>("", "parent_commit_id")
            .unwrap(),
        source_head
    );
    assert_eq!(
        store
            .merge_context_at(command, 1_700_000_700_001)
            .await
            .unwrap(),
        merged
    );
}

#[tokio::test]
async fn conflicting_merge_persists_then_applies_explicit_resolution() {
    let store = store().await;
    let root = temporary_root(&store, "conflict").await;
    let source = fork(&store, &root, "conflict-source").await;
    let target = fork(&store, &root, "conflict-target").await;
    let source_head = commit(
        &store,
        &root.0,
        &source.branch_id,
        &source.head_commit_id,
        "value",
        1,
    )
    .await;
    let target_head = commit(
        &store,
        &root.0,
        &target.branch_id,
        &target.head_commit_id,
        "value",
        2,
    )
    .await;
    let mut command = MergeContextCommand {
        context_id: root.0.clone(),
        source_branch_id: source.branch_id.clone(),
        target_branch_id: target.branch_id.clone(),
        expected_source_head: source_head,
        expected_target_head: target_head,
        source_disposition: MergeSourceDisposition::MarkMerged,
        selections: vec![],
        idempotency_key: "conflicted-merge".into(),
    };
    let conflicted = store
        .merge_context_at(command.clone(), 1_700_000_710_000)
        .await
        .unwrap();
    assert_eq!(conflicted.status, MergeContextStatus::Conflicted);
    assert_eq!(conflicted.conflicts.len(), 1);
    assert_eq!(conflicted.conflicts[0].path, "/value");
    let conflict = conflicted.conflicts[0].clone();
    command.selections = vec![ExplicitMergeSelection {
        conflict_id: conflict.conflict_id,
        path: conflict.path,
        resolution: ExplicitMergeResolution::FinalValue { value: json!(3) },
    }];
    command.idempotency_key = "resolved-merge".into();
    let merged = store
        .merge_context_at(command, 1_700_000_710_001)
        .await
        .unwrap();
    assert_eq!(merged.status, MergeContextStatus::Merged);
    assert_eq!(
        store
            .get_working_context(&root.0, &target.branch_id)
            .await
            .unwrap()
            .value,
        json!({"value":3})
    );
    let source_status = store
        .db
        .query_one_raw(sql(
            "SELECT status FROM context_branches WHERE id = ?",
            vec![source.branch_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "status")
        .unwrap();
    assert_eq!(source_status, "merged");
    let conflict_status = store
        .db
        .query_one_raw(sql(
            "SELECT status FROM context_merge_conflicts LIMIT 1",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "status")
        .unwrap();
    assert_eq!(conflict_status, "resolved");
}
