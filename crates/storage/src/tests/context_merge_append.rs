use serde_json::json;
use zhuangsheng_core::context_merge::{
    MergeContextCommand, MergeContextStatus, MergeSourceDisposition,
};

use super::{
    context_merge_support::{append, commit_value, fork, temporary_root},
    store,
};

#[tokio::test]
async fn merge_deduplicates_verified_appends_and_preserves_ids_for_replay() {
    let store = store().await;
    let initial = temporary_root(&store, "append").await;
    let root_head = commit_value(
        &store,
        &initial.0,
        &initial.1,
        &initial.2,
        "items",
        json!(["base"]),
    )
    .await;
    let root = (initial.0, initial.1, root_head);
    let source = fork(&store, &root, "append-source").await;
    let target = fork(&store, &root, "append-target").await;
    let source_head = append(
        &store,
        &root.0,
        &source.branch_id,
        &source.head_commit_id,
        "source-element",
        json!("source"),
    )
    .await;
    let target_head = append(
        &store,
        &root.0,
        &target.branch_id,
        &target.head_commit_id,
        "target-element",
        json!("target"),
    )
    .await;
    let merged = store
        .merge_context_at(
            MergeContextCommand {
                context_id: root.0.clone(),
                source_branch_id: source.branch_id.clone(),
                target_branch_id: target.branch_id.clone(),
                expected_source_head: source_head,
                expected_target_head: target_head,
                source_disposition: MergeSourceDisposition::KeepActive,
                selections: vec![],
                idempotency_key: "append-merge".into(),
            },
            1_700_000_720_000,
        )
        .await
        .unwrap();
    assert_eq!(merged.status, MergeContextStatus::Merged);
    let expected = if source.branch_id < target.branch_id {
        json!(["base", "source", "target"])
    } else {
        json!(["base", "target", "source"])
    };
    let value = store
        .get_working_context(&root.0, &target.branch_id)
        .await
        .unwrap()
        .value;
    assert_eq!(value["items"], expected);

    let duplicate_head = append(
        &store,
        &root.0,
        &target.branch_id,
        merged.merge_commit_id.as_deref().unwrap(),
        "source-element",
        json!("duplicate-must-not-appear"),
    )
    .await;
    let replayed = store.get_context_at_commit(&duplicate_head).await.unwrap();
    assert_eq!(replayed.value, value);
}
