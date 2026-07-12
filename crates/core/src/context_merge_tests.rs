use serde_json::json;

use crate::context_merge::analyze_three_way;

#[test]
fn merges_disjoint_equal_and_append_only_changes() {
    let analysis = analyze_three_way(
        &json!({"left":0,"right":0,"items":[{"id":"base"}]}),
        &json!({"left":1,"right":0,"items":[{"id":"base"},{"id":"source"}]}),
        &json!({"left":0,"right":2,"items":[{"id":"base"},{"id":"target"}]}),
    );
    assert!(analysis.conflicts.is_empty());
    assert_eq!(
        analysis.merged,
        json!({
            "left":1,"right":2,
            "items":[{"id":"base"},{"id":"source"},{"id":"target"}]
        })
    );
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
