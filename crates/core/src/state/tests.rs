use serde_json::json;

use super::*;

fn patch(ops: Vec<JsonPatchOp>) -> StatePatch {
    StatePatch {
        aggregate_kind: AggregateKind::WorkingContext,
        aggregate_id: "context".into(),
        lineage_key: "branch".into(),
        base_commit_id: "commit-1".into(),
        operation_id: "operation-1".into(),
        ops,
        schema_version: 1,
        policy_version: 1,
        author: ActorRef {
            kind: ActorKind::Node,
            id: Some("node-1".into()),
        },
    }
}

#[test]
fn applies_closed_patch_operations_in_order() {
    let patch = patch(vec![
        JsonPatchOp::Test {
            path: "/status".into(),
            value: json!("draft"),
        },
        JsonPatchOp::Replace {
            path: "/status".into(),
            value: json!("ready"),
        },
        JsonPatchOp::Add {
            path: "/scene".into(),
            value: json!({"name":"hall"}),
        },
        JsonPatchOp::Append {
            path: "/messages".into(),
            element_id: "message-1".into(),
            value: json!({"id":"message-1","text":"hello"}),
        },
        JsonPatchOp::Remove {
            path: "/obsolete".into(),
        },
    ]);
    let result = apply_patch(
        &json!({"status":"draft","messages":[],"obsolete":true}),
        &patch,
    )
    .unwrap();
    assert_eq!(
        result,
        json!({
            "status":"ready",
            "scene":{"name":"hall"},
            "messages":[{"id":"message-1","text":"hello"}]
        })
    );
}

#[test]
fn rejects_missing_paths_invalid_pointers_and_failed_tests() {
    for operation in [
        JsonPatchOp::Replace {
            path: "/missing".into(),
            value: json!(1),
        },
        JsonPatchOp::Add {
            path: "missing".into(),
            value: json!(1),
        },
        JsonPatchOp::Test {
            path: "/value".into(),
            value: json!(2),
        },
    ] {
        assert!(apply_patch(&json!({"value":1}), &patch(vec![operation])).is_err());
    }
}

#[test]
fn detects_prefix_conflicts_but_allows_concurrent_appends() {
    let left = patch(vec![JsonPatchOp::Replace {
        path: "/scene".into(),
        value: json!({}),
    }]);
    let nested = patch(vec![JsonPatchOp::Add {
        path: "/scene/name".into(),
        value: json!("hall"),
    }]);
    assert!(patches_conflict(&left, &nested));

    let append = |id: &str| {
        patch(vec![JsonPatchOp::Append {
            path: "/messages".into(),
            element_id: id.into(),
            value: json!({"id":id}),
        }])
    };
    assert!(!patches_conflict(&append("a"), &append("b")));
}
