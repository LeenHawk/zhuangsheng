use std::collections::BTreeSet;

use crate::ValidationIssue;

use super::{DraftNodeKind, GraphNode, InputSelector, RunLimits};

pub(super) fn validate_coordination(
    node: &GraphNode,
    limits: &RunLimits,
    issues: &mut Vec<ValidationIssue>,
) {
    match &node.kind {
        DraftNodeKind::Merge { .. } => {
            if node.inputs.len() < 2 || node.outputs.len() != 1 {
                push(issues, "invalid_merge_shape", node, None);
            }
        }
        DraftNodeKind::JoinByKey {
            key_selectors,
            max_open_keys,
            max_buffered_per_key_per_port,
        } => {
            if node.inputs.len() < 2 || node.outputs.len() != 1 {
                push(issues, "invalid_join_by_key_shape", node, None);
            }
            let ports: BTreeSet<_> = node.inputs.iter().map(|input| &input.name).collect();
            let selectors: BTreeSet<_> = key_selectors.keys().collect();
            if ports != selectors {
                push(
                    issues,
                    "join_key_selectors_mismatch",
                    node,
                    Some("keySelectors"),
                );
            }
            for (port, pointer) in key_selectors {
                if crate::selector::validate(&InputSelector::JsonPointer {
                    pointer: pointer.clone(),
                })
                .is_err()
                {
                    push(
                        issues,
                        "invalid_join_key_selector",
                        node,
                        Some(&format!("keySelectors/{port}")),
                    );
                }
            }
            if *max_open_keys == 0
                || *max_buffered_per_key_per_port == 0
                || *max_open_keys > limits.max_coordinator_buffered_values
                || *max_buffered_per_key_per_port > limits.max_coordinator_buffered_values
            {
                push(issues, "invalid_join_by_key_limits", node, None);
            }
        }
        _ => {}
    }
}

fn push(
    issues: &mut Vec<ValidationIssue>,
    code: &'static str,
    node: &GraphNode,
    suffix: Option<&str>,
) {
    let path = suffix.map_or_else(
        || format!("/nodes/{}", node.id),
        |suffix| format!("/nodes/{}/{suffix}", node.id),
    );
    issues.push(ValidationIssue::error(code, path, code.replace('_', " ")));
}
