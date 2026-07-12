use serde_json::json;
use zhuangsheng_core::context_merge::{
    ExplicitMergeResolution, ExplicitMergeSelection, MergeContextCommand, MergeContextStatus,
    MergeSourceDisposition,
};

use crate::StorageError;

use super::{
    context_merge_support::{commit, fork, temporary_root},
    store,
};

#[tokio::test]
async fn merge_rejects_distinct_branches_with_the_same_head() {
    let store = store().await;
    let root = temporary_root(&store, "same-head").await;
    let source = fork(&store, &root, "same-head-source").await;
    let target = fork(&store, &root, "same-head-target").await;
    let error = store
        .merge_context_at(
            MergeContextCommand {
                context_id: root.0,
                source_branch_id: source.branch_id,
                target_branch_id: target.branch_id,
                expected_source_head: root.2.clone(),
                expected_target_head: root.2,
                source_disposition: MergeSourceDisposition::KeepActive,
                selections: vec![],
                idempotency_key: "same-head-merge".into(),
            },
            1_700_000_730_000,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::Conflict("merge_heads_identical")
    ));
}

#[tokio::test]
async fn partial_selections_return_all_conflicts_without_a_merge_commit() {
    let store = store().await;
    let root = temporary_root(&store, "partial").await;
    let source = fork(&store, &root, "partial-source").await;
    let target = fork(&store, &root, "partial-target").await;
    let source_a = commit(
        &store,
        &root.0,
        &source.branch_id,
        &source.head_commit_id,
        "a",
        1,
    )
    .await;
    let source_head = commit(&store, &root.0, &source.branch_id, &source_a, "b", 1).await;
    let target_a = commit(
        &store,
        &root.0,
        &target.branch_id,
        &target.head_commit_id,
        "a",
        2,
    )
    .await;
    let target_head = commit(&store, &root.0, &target.branch_id, &target_a, "b", 2).await;
    let command = |key: &str, selections| MergeContextCommand {
        context_id: root.0.clone(),
        source_branch_id: source.branch_id.clone(),
        target_branch_id: target.branch_id.clone(),
        expected_source_head: source_head.clone(),
        expected_target_head: target_head.clone(),
        source_disposition: MergeSourceDisposition::KeepActive,
        selections,
        idempotency_key: key.into(),
    };
    let conflicted = store
        .merge_context_at(command("partial-analysis", vec![]), 1_700_000_731_000)
        .await
        .unwrap();
    assert_eq!(conflicted.conflicts.len(), 2);
    let selected = conflicted.conflicts[0].clone();
    let partial = store
        .merge_context_at(
            command(
                "partial-resolution",
                vec![ExplicitMergeSelection {
                    conflict_id: selected.conflict_id,
                    path: selected.path,
                    resolution: ExplicitMergeResolution::FinalValue { value: json!(3) },
                }],
            ),
            1_700_000_731_001,
        )
        .await
        .unwrap();
    assert_eq!(partial.status, MergeContextStatus::Conflicted);
    assert_eq!(partial.conflicts.len(), 2);
    assert_eq!(partial.merge_commit_id, None);
    assert_eq!(
        store
            .get_working_context(&root.0, &target.branch_id)
            .await
            .unwrap()
            .head_commit_id,
        target_head
    );
}
