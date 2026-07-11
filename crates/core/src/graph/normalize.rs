use std::collections::{HashSet, VecDeque};

use crate::ValidationIssue;

use super::{DraftRunLimits, GraphEdge, GraphNode, RunLimits};

pub(super) fn normalize_limits(
    draft: Option<DraftRunLimits>,
    issues: &mut Vec<ValidationIssue>,
) -> RunLimits {
    let defaults = RunLimits::default();
    let draft = draft.unwrap_or_default();
    let limits = RunLimits {
        max_node_activations: draft
            .max_node_activations
            .unwrap_or(defaults.max_node_activations),
        max_attempts_per_activation: draft
            .max_attempts_per_activation
            .unwrap_or(defaults.max_attempts_per_activation),
        max_total_queue_values: draft
            .max_total_queue_values
            .unwrap_or(defaults.max_total_queue_values),
        max_pending_queue_values: draft
            .max_pending_queue_values
            .unwrap_or(defaults.max_pending_queue_values),
        max_open_waits: draft.max_open_waits.unwrap_or(defaults.max_open_waits),
        max_coordinator_buffered_values: draft
            .max_coordinator_buffered_values
            .unwrap_or(defaults.max_coordinator_buffered_values),
        max_run_wall_clock_ms: draft
            .max_run_wall_clock_ms
            .unwrap_or(defaults.max_run_wall_clock_ms),
        max_value_bytes: draft.max_value_bytes.unwrap_or(defaults.max_value_bytes),
    };
    if limits.max_node_activations == 0
        || limits.max_attempts_per_activation == 0
        || limits.max_total_queue_values == 0
        || limits.max_pending_queue_values == 0
        || limits.max_open_waits == 0
        || limits.max_coordinator_buffered_values == 0
        || limits.max_run_wall_clock_ms == 0
        || limits.max_value_bytes == 0
    {
        issues.push(ValidationIssue::error(
            "run_limit_not_positive",
            "/limits",
            "run limit not positive",
        ));
    }
    limits
}

pub(super) fn unreachable_warnings(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Vec<ValidationIssue> {
    let mut reached: HashSet<_> = nodes
        .iter()
        .filter(|node| node.is_entry)
        .map(|node| node.id.clone())
        .collect();
    let mut queue: VecDeque<_> = reached.iter().cloned().collect();
    while let Some(id) = queue.pop_front() {
        for edge in edges.iter().filter(|edge| edge.from.node_id == id) {
            if reached.insert(edge.to.node_id.clone()) {
                queue.push_back(edge.to.node_id.clone());
            }
        }
    }
    nodes
        .iter()
        .filter(|node| !reached.contains(&node.id))
        .map(|node| {
            ValidationIssue::error(
                "node_unreachable",
                format!("/nodes/{}", node.id),
                "node is not reachable from an input node",
            )
        })
        .collect()
}
