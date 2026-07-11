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
