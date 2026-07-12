use serde_json::json;

use crate::{DomainError, graph::*};

fn valid() -> GraphDraft {
    GraphDraft {
        graph_id: "graph-1".into(),
        name: Some("basic".into()),
        nodes: vec![
            DraftGraphNode {
                id: "input".into(),
                name: None,
                is_entry: None,
                inputs: vec![],
                outputs: vec![],
                timeout_ms: None,
                retry_policy: None,
                kind: DraftNodeKind::Input {
                    run_input_selector: Default::default(),
                },
            },
            DraftGraphNode {
                id: "output".into(),
                name: None,
                is_entry: None,
                inputs: vec![],
                outputs: vec![],
                timeout_ms: None,
                retry_policy: None,
                kind: DraftNodeKind::Output {
                    output_key: "reply".into(),
                },
            },
        ],
        edges: vec![DraftGraphEdge {
            id: None,
            from: GraphOutputRef {
                node_id: "input".into(),
                output: "default".into(),
            },
            to: GraphInputRef {
                node_id: "output".into(),
                input: "default".into(),
            },
        }],
        run_input_schema: None,
        output_contract: vec![GraphOutputContractEntry {
            key: "reply".into(),
            schema: None,
            collection: OutputCollection::Single,
            required: true,
        }],
        limits: Some(DraftRunLimits::default()),
    }
}

#[test]
fn normalizes_and_hashes_graph() {
    let applied = apply_graph(valid(), 1, 1).unwrap();
    assert_eq!(applied.definition.nodes[0].outputs[0].name, "default");
    assert!(applied.content_hash.starts_with("sha256:"));
    assert!(applied.definition.edges[0].id.starts_with("edge_"));
}

#[test]
fn rejects_unconnected_output() {
    let mut draft = valid();
    draft.edges.clear();
    assert!(matches!(
        apply_graph(draft, 1, 1),
        Err(DomainError::GraphValidation(_))
    ));
}

#[test]
fn merge_any_requires_two_inputs_and_materializes_one_output() {
    let draft: GraphDraft = serde_json::from_value(json!({
        "graphId":"graph-merge",
        "nodes":[
            {"id":"left","kind":"input"},
            {"id":"right","kind":"input"},
            {
                "id":"merge",
                "kind":"merge",
                "mode":"any",
                "inputs":[{"name":"left"},{"name":"right"}]
            },
            {"id":"output","kind":"output","outputKey":"items"}
        ],
        "edges":[
            {"from":{"nodeId":"left","output":"default"},"to":{"nodeId":"merge","input":"left"}},
            {"from":{"nodeId":"right","output":"default"},"to":{"nodeId":"merge","input":"right"}},
            {"from":{"nodeId":"merge","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "outputContract":[{"key":"items","collection":"append","required":true}]
    }))
    .unwrap();
    let applied = apply_graph(draft.clone(), 1, 1).unwrap();
    let merge = applied
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "merge")
        .unwrap();
    assert_eq!(merge.inputs.len(), 2);
    assert_eq!(merge.outputs[0].name, "default");

    let mut invalid = draft;
    invalid
        .nodes
        .iter_mut()
        .find(|node| node.id == "merge")
        .unwrap()
        .inputs
        .pop();
    invalid.edges.retain(|edge| edge.to.input != "right");
    let DomainError::GraphValidation(issues) = apply_graph(invalid, 1, 1).unwrap_err() else {
        panic!("expected graph validation")
    };
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "invalid_merge_shape")
    );
}

#[test]
fn join_by_key_requires_complete_valid_selectors_and_bounded_limits() {
    let draft: GraphDraft = serde_json::from_value(json!({
        "graphId":"graph-join",
        "nodes":[
            {"id":"left","kind":"input"},
            {"id":"right","kind":"input"},
            {
                "id":"join",
                "kind":"join_by_key",
                "inputs":[{"name":"left"},{"name":"right"}],
                "keySelectors":{"left":"/id","right":"/id"},
                "maxOpenKeys":8,
                "maxBufferedPerKeyPerPort":4
            },
            {"id":"output","kind":"output","outputKey":"items"}
        ],
        "edges":[
            {"from":{"nodeId":"left","output":"default"},"to":{"nodeId":"join","input":"left"}},
            {"from":{"nodeId":"right","output":"default"},"to":{"nodeId":"join","input":"right"}},
            {"from":{"nodeId":"join","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "outputContract":[{"key":"items","collection":"append","required":true}],
        "limits":{"maxCoordinatorBufferedValues":8}
    }))
    .unwrap();
    let applied = apply_graph(draft.clone(), 1, 1).unwrap();
    assert_eq!(applied.definition.nodes[2].outputs[0].name, "default");

    let mut invalid = draft;
    let DraftNodeKind::JoinByKey {
        key_selectors,
        max_open_keys,
        ..
    } = &mut invalid.nodes[2].kind
    else {
        unreachable!()
    };
    key_selectors.remove("right");
    key_selectors.insert("left".into(), "not-a-pointer".into());
    *max_open_keys = 9;
    let DomainError::GraphValidation(issues) = apply_graph(invalid, 1, 1).unwrap_err() else {
        panic!("expected graph validation")
    };
    for code in [
        "join_key_selectors_mismatch",
        "invalid_join_key_selector",
        "invalid_join_by_key_limits",
    ] {
        assert!(issues.iter().any(|issue| issue.code == code), "{code}");
    }
}
