use serde_json::json;

use crate::context_merge::{MergeAppendItem, analyze_three_way, merge_append_only_arrays};

#[test]
fn merges_disjoint_and_equal_changes() {
    let analysis = analyze_three_way(
        &json!({"left":0,"right":0,"equal":0}),
        &json!({"left":1,"right":0,"equal":3}),
        &json!({"left":0,"right":2,"equal":3}),
    );
    assert!(analysis.conflicts.is_empty());
    assert_eq!(analysis.merged, json!({"left":1,"right":2,"equal":3}));
}

#[test]
fn merges_verified_appends_by_stable_origin_and_deduplicates_ids() {
    let item = |branch: &str, id: &str, value| MergeAppendItem {
        path: "/items".into(),
        element_id: id.into(),
        value,
        branch_id: branch.into(),
        sequence_no: 2,
        operation_id: format!("append-{id}"),
        operation_index: 0,
    };
    let source = vec![item("z-source", "shared", json!("same"))];
    let target = vec![
        item("a-target", "target", json!("target")),
        item("a-target", "shared", json!("same")),
    ];
    let merged = merge_append_only_arrays(
        "/items",
        &json!(["base"]),
        &json!(["base", "same"]),
        &json!(["base", "target", "same"]),
        &source,
        &target,
    )
    .unwrap();
    assert_eq!(merged.value, json!(["base", "same", "target"]));
    assert!(merged.integrated_source.is_empty());
}

#[test]
fn unverified_array_tails_conflict() {
    let analysis = analyze_three_way(
        &json!({"items":["base"]}),
        &json!({"items":["base","source"]}),
        &json!({"items":["base","target"]}),
    );
    assert_eq!(analysis.conflicts[0].path, "/items");
}

#[test]
fn reports_stable_pointer_for_overlapping_change() {
    let analysis = analyze_three_way(
        &json!({"nested":{"value":0}}),
        &json!({"nested":{"value":1}}),
        &json!({"nested":{"value":2}}),
    );
    assert_eq!(analysis.conflicts.len(), 1);
    assert_eq!(analysis.conflicts[0].path, "/nested/value");
    assert_eq!(analysis.merged, json!({"nested":{"value":2}}));
}

#[test]
fn merges_deletion_when_only_one_side_changed() {
    let analysis = analyze_three_way(
        &json!({"keep":1,"remove":2}),
        &json!({"keep":1}),
        &json!({"keep":1,"remove":2}),
    );
    assert!(analysis.conflicts.is_empty());
    assert_eq!(analysis.merged, json!({"keep":1}));
}
