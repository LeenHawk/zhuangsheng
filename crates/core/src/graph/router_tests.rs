use crate::{DomainError, graph::*};

fn draft() -> GraphDraft {
    GraphDraft {
        graph_id: "router-graph".into(),
        name: None,
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
                id: "router".into(),
                name: None,
                is_entry: None,
                inputs: vec![],
                outputs: vec![OutputPortDefinition {
                    name: "done".into(),
                    schema: None,
                }],
                timeout_ms: None,
                retry_policy: None,
                kind: DraftNodeKind::Router {
                    dsl_version: "router-dsl-v1".into(),
                    rules: vec![RouterRule {
                        id: "always".into(),
                        when: "inputs.default != null".into(),
                        outputs: vec!["done".into()],
                    }],
                    match_mode: RouterMatchMode::First,
                    default_outputs: None,
                    payload_port: Some("default".into()),
                    memory: None,
                    limits: None,
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
        edges: vec![
            edge("input", "default", "router", "default"),
            edge("router", "done", "output", "default"),
        ],
        run_input_schema: None,
        output_contract: vec![GraphOutputContractEntry {
            key: "reply".into(),
            schema: None,
            collection: OutputCollection::Single,
            required: true,
        }],
        limits: None,
    }
}

#[test]
fn apply_compiles_router_rules_and_normalizes_reconcile_limit() {
    let applied = apply_graph(draft(), 1, 1).unwrap();
    let router = &applied.definition.nodes[1];
    let DraftNodeKind::Router {
        limits: Some(limits),
        ..
    } = &router.kind
    else {
        panic!("normalized Router limits missing")
    };
    assert_eq!(limits.max_read_reconciles, Some(2));
}

#[test]
fn apply_rejects_invalid_router_expression_route_and_limits() {
    let mut invalid = draft();
    let DraftNodeKind::Router { rules, limits, .. } = &mut invalid.nodes[1].kind else {
        unreachable!()
    };
    rules[0].when = "size(inputs.default)".into();
    rules[0].outputs.clear();
    *limits = Some(RouterLimits {
        max_visits_per_run: Some(0),
        timeout_ms_per_run: None,
        max_read_reconciles: Some(8),
        on_limit_outputs: None,
    });
    let Err(DomainError::GraphValidation(issues)) = apply_graph(invalid, 1, 1) else {
        panic!("invalid Router unexpectedly applied")
    };
    for code in [
        "router_when_not_boolean",
        "router_output_invalid",
        "invalid_router_limits",
    ] {
        assert!(issues.iter().any(|issue| issue.code == code), "{code}");
    }
}

#[test]
fn apply_normalizes_and_validates_router_memory_reads() {
    let mut graph = draft();
    let DraftNodeKind::Router { memory, .. } = &mut graph.nodes[1].kind else {
        unreachable!()
    };
    *memory = Some(RouterMemoryBinding {
        reads: vec![RouterReadBinding {
            id: "scene-read".into(),
            alias: "scene".into(),
            source: RouterReadSource::LongTermMemory {
                scope: "story".into(),
                query: Some(MemoryQuery {
                    text: "current scene".into(),
                    tags: vec!["scene".into(), "current".into(), "scene".into()],
                    status: Some(MemoryRecordStatus::Active),
                }),
            },
            required: true,
            consistency: MemoryReadConsistency::Snapshot,
            limit: None,
            max_bytes: 1024,
        }],
    });
    let applied = apply_graph(graph, 1, 1).unwrap();
    let DraftNodeKind::Router {
        memory: Some(memory),
        ..
    } = &applied.definition.nodes[1].kind
    else {
        panic!("Router memory missing")
    };
    assert_eq!(memory.reads[0].limit, Some(20));
    let RouterReadSource::LongTermMemory {
        query: Some(query), ..
    } = &memory.reads[0].source
    else {
        panic!("query missing")
    };
    assert_eq!(query.tags, ["current", "scene"]);

    let mut invalid = draft();
    let DraftNodeKind::Router { memory, .. } = &mut invalid.nodes[1].kind else {
        unreachable!()
    };
    *memory = Some(RouterMemoryBinding {
        reads: vec![RouterReadBinding {
            id: "read".into(),
            alias: "scene".into(),
            source: RouterReadSource::WorkingContext {
                scope: "story".into(),
                path: "not-a-pointer".into(),
            },
            required: true,
            consistency: MemoryReadConsistency::ValidateOnCommit,
            limit: Some(1),
            max_bytes: 1024,
        }],
    });
    let Err(DomainError::GraphValidation(issues)) = apply_graph(invalid, 1, 1) else {
        panic!("invalid memory read applied")
    };
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "invalid_router_memory_read")
    );
}

fn edge(from_node: &str, from: &str, to_node: &str, to: &str) -> DraftGraphEdge {
    DraftGraphEdge {
        id: None,
        from: GraphOutputRef {
            node_id: from_node.into(),
            output: from.into(),
        },
        to: GraphInputRef {
            node_id: to_node.into(),
            input: to.into(),
        },
    }
}
