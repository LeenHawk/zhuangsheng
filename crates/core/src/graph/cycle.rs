use std::collections::{HashMap, HashSet};

use crate::ValidationIssue;

use super::{DraftNodeKind, GraphEdge, GraphNode};

pub fn validate_cycles(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    issues: &mut Vec<ValidationIssue>,
) {
    let adjacency = adjacency(nodes, edges);
    for component in strongly_connected(nodes, &adjacency) {
        let cyclic = component.len() > 1
            || adjacency
                .get(&component[0])
                .is_some_and(|targets| targets.contains(&component[0]));
        if !cyclic {
            continue;
        }
        let guarded: HashSet<_> = component
            .iter()
            .filter(|id| node(nodes, id).is_some_and(is_guarded_router))
            .cloned()
            .collect();
        if guarded.is_empty() {
            issues.push(issue("cycle_without_router_guard", &component));
            continue;
        }
        let remaining: HashSet<_> = component
            .iter()
            .filter(|id| !guarded.contains(*id))
            .cloned()
            .collect();
        if contains_cycle(&remaining, &adjacency) {
            issues.push(issue("cycle_bypasses_router_guard", &component));
        }
        let component_set: HashSet<_> = component.iter().cloned().collect();
        for guard in guarded {
            let Some(GraphNode {
                kind:
                    DraftNodeKind::Router {
                        limits: Some(limits),
                        ..
                    },
                ..
            }) = node(nodes, &guard)
            else {
                continue;
            };
            for output in &limits.on_limit_outputs {
                let leaves = edges
                    .iter()
                    .filter(|edge| edge.from.node_id == guard && edge.from.output == *output)
                    .all(|edge| !component_set.contains(&edge.to.node_id));
                if !leaves {
                    issues.push(ValidationIssue::error(
                        "router_limit_route_reenters_cycle",
                        format!("/nodes/{guard}/limits/onLimitOutputs"),
                        "router limit output must leave its cyclic component",
                    ));
                }
            }
        }
    }
}

fn adjacency(nodes: &[GraphNode], edges: &[GraphEdge]) -> HashMap<String, Vec<String>> {
    let mut result: HashMap<_, Vec<_>> = nodes
        .iter()
        .map(|node| (node.id.clone(), Vec::new()))
        .collect();
    for edge in edges {
        result
            .entry(edge.from.node_id.clone())
            .or_default()
            .push(edge.to.node_id.clone());
    }
    result
}

fn strongly_connected(
    nodes: &[GraphNode],
    adjacency: &HashMap<String, Vec<String>>,
) -> Vec<Vec<String>> {
    struct Tarjan<'a> {
        next: usize,
        stack: Vec<String>,
        on_stack: HashSet<String>,
        index: HashMap<String, usize>,
        low: HashMap<String, usize>,
        graph: &'a HashMap<String, Vec<String>>,
        output: Vec<Vec<String>>,
    }
    fn visit(id: &str, state: &mut Tarjan<'_>) {
        let index = state.next;
        state.next += 1;
        state.index.insert(id.into(), index);
        state.low.insert(id.into(), index);
        state.stack.push(id.into());
        state.on_stack.insert(id.into());
        for target in state.graph.get(id).into_iter().flatten() {
            if !state.index.contains_key(target) {
                visit(target, state);
                let low = state.low[id].min(state.low[target]);
                state.low.insert(id.into(), low);
            } else if state.on_stack.contains(target) {
                state
                    .low
                    .insert(id.into(), state.low[id].min(state.index[target]));
            }
        }
        if state.low[id] == state.index[id] {
            let mut component = Vec::new();
            while let Some(value) = state.stack.pop() {
                state.on_stack.remove(&value);
                component.push(value.clone());
                if value == id {
                    break;
                }
            }
            component.sort();
            state.output.push(component);
        }
    }
    let mut state = Tarjan {
        next: 0,
        stack: Vec::new(),
        on_stack: HashSet::new(),
        index: HashMap::new(),
        low: HashMap::new(),
        graph: adjacency,
        output: Vec::new(),
    };
    for node in nodes {
        if !state.index.contains_key(&node.id) {
            visit(&node.id, &mut state);
        }
    }
    state.output
}

fn contains_cycle(nodes: &HashSet<String>, adjacency: &HashMap<String, Vec<String>>) -> bool {
    fn visit(
        id: &str,
        nodes: &HashSet<String>,
        graph: &HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> bool {
        if visiting.contains(id) {
            return true;
        }
        if done.contains(id) {
            return false;
        }
        visiting.insert(id.into());
        for target in graph
            .get(id)
            .into_iter()
            .flatten()
            .filter(|target| nodes.contains(*target))
        {
            if visit(target, nodes, graph, visiting, done) {
                return true;
            }
        }
        visiting.remove(id);
        done.insert(id.into());
        false
    }
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    nodes
        .iter()
        .any(|id| visit(id, nodes, adjacency, &mut visiting, &mut done))
}

fn node<'a>(nodes: &'a [GraphNode], id: &str) -> Option<&'a GraphNode> {
    nodes.iter().find(|node| node.id == id)
}

fn is_guarded_router(node: &GraphNode) -> bool {
    matches!(&node.kind, DraftNodeKind::Router { limits: Some(limits), .. } if limits.max_visits_per_run.is_some() || limits.timeout_ms_per_run.is_some())
}

fn issue(code: &str, component: &[String]) -> ValidationIssue {
    ValidationIssue::error(
        code,
        "/edges",
        format!("{}: {}", code.replace('_', " "), component.join(", ")),
    )
}
