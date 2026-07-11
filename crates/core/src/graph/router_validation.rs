use std::collections::HashSet;

use crate::{ValidationIssue, router::compile_expression};

use super::{DraftNodeKind, GraphNode, RouterLimits, RouterReadSource, RunLimits};

const MAX_ROUTER_RULES: usize = 128;
const DEFAULT_MAX_READ_RECONCILES: u64 = 2;

pub(super) fn normalize_router(kind: &mut DraftNodeKind) {
    let DraftNodeKind::Router { limits, memory, .. } = kind else {
        return;
    };
    let limits = limits.get_or_insert_with(RouterLimits::default);
    limits
        .max_read_reconciles
        .get_or_insert(DEFAULT_MAX_READ_RECONCILES);
    if let Some(memory) = memory {
        for read in &mut memory.reads {
            if matches!(&read.source, RouterReadSource::LongTermMemory { .. }) {
                read.limit.get_or_insert(20);
            }
            if let RouterReadSource::LongTermMemory {
                query: Some(query), ..
            } = &mut read.source
            {
                query.tags.sort();
                query.tags.dedup();
            }
        }
    }
}

pub(super) fn validate_router(
    node: &GraphNode,
    run_limits: &RunLimits,
    issues: &mut Vec<ValidationIssue>,
) {
    let DraftNodeKind::Router {
        dsl_version,
        rules,
        default_outputs,
        payload_port,
        memory,
        limits,
        ..
    } = &node.kind
    else {
        return;
    };
    if dsl_version != "router-dsl-v1" {
        push(issues, "unsupported_router_dsl", &node.id, "dslVersion");
    }
    if rules.len() > MAX_ROUTER_RULES {
        push(issues, "router_too_many_rules", &node.id, "rules");
    }
    let outputs: HashSet<_> = node.outputs.iter().map(|port| port.name.as_str()).collect();
    let mut rule_ids = HashSet::new();
    for (index, rule) in rules.iter().enumerate() {
        if rule.id.is_empty() || !rule_ids.insert(&rule.id) {
            push(issues, "duplicate_router_rule", &node.id, "rules");
        }
        validate_route(
            &rule.outputs,
            &outputs,
            true,
            issues,
            format!("/nodes/{}/rules/{index}/outputs", node.id),
        );
        match compile_expression(&rule.when) {
            Ok(expression) if expression.is_statically_non_boolean() => {
                issues.push(ValidationIssue::error(
                    "router_when_not_boolean",
                    format!("/nodes/{}/rules/{index}/when", node.id),
                    "Router rule expression is statically non-boolean",
                ))
            }
            Ok(_) => {}
            Err(error) => issues.push(ValidationIssue::error(
                error.code,
                format!("/nodes/{}/rules/{index}/when", node.id),
                error.message,
            )),
        }
    }
    if let Some(default_outputs) = default_outputs {
        validate_route(
            default_outputs,
            &outputs,
            true,
            issues,
            format!("/nodes/{}/defaultOutputs", node.id),
        );
    }
    if let Some(port) = payload_port
        && !node.inputs.iter().any(|input| input.name == *port)
    {
        push(
            issues,
            "router_payload_port_missing",
            &node.id,
            "payloadPort",
        );
    }
    if let Some(limits) = limits {
        validate_limits(node, limits, run_limits, &outputs, issues);
    }
    if let Some(memory) = memory {
        validate_memory(node, &memory.reads, issues);
    }
}

fn validate_memory(
    node: &GraphNode,
    reads: &[super::RouterReadBinding],
    issues: &mut Vec<ValidationIssue>,
) {
    if reads.len() > 128 {
        push(
            issues,
            "invalid_router_memory_read",
            &node.id,
            "memory/reads",
        );
    }
    let mut ids = HashSet::new();
    let mut aliases = HashSet::new();
    for (index, read) in reads.iter().enumerate() {
        let invalid_identity = read.id.is_empty()
            || read.alias.is_empty()
            || !ids.insert(&read.id)
            || !aliases.insert(&read.alias);
        let invalid_source = match &read.source {
            RouterReadSource::WorkingContext { scope, path } => {
                scope.is_empty()
                    || read.limit.is_some()
                    || crate::selector::validate(&super::InputSelector::JsonPointer {
                        pointer: path.clone(),
                    })
                    .is_err()
            }
            RouterReadSource::LongTermMemory { scope, query } => {
                scope.is_empty()
                    || read.limit.is_none_or(|limit| limit == 0 || limit > 100)
                    || query.as_ref().is_some_and(|query| {
                        query.text.len() > 4096
                            || query.tags.len() > 64
                            || query.tags.iter().any(|tag| tag.is_empty())
                    })
            }
        };
        if invalid_identity || invalid_source || read.max_bytes == 0 || read.max_bytes > 1024 * 1024
        {
            issues.push(ValidationIssue::error(
                "invalid_router_memory_read",
                format!("/nodes/{}/memory/reads/{index}", node.id),
                "Router memory read is invalid",
            ));
        }
    }
}

fn validate_limits(
    node: &GraphNode,
    limits: &RouterLimits,
    run_limits: &RunLimits,
    outputs: &HashSet<&str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let invalid = limits.max_visits_per_run == Some(0)
        || limits.timeout_ms_per_run == Some(0)
        || limits
            .max_read_reconciles
            .is_none_or(|value| value == 0 || value >= run_limits.max_attempts_per_activation);
    if invalid {
        push(issues, "invalid_router_limits", &node.id, "limits");
    }
    if let Some(on_limit_outputs) = &limits.on_limit_outputs {
        validate_route(
            on_limit_outputs,
            outputs,
            true,
            issues,
            format!("/nodes/{}/limits/onLimitOutputs", node.id),
        );
    }
}

fn validate_route(
    values: &[String],
    outputs: &HashSet<&str>,
    require_nonempty: bool,
    issues: &mut Vec<ValidationIssue>,
    path: String,
) {
    let mut seen = HashSet::new();
    if (require_nonempty && values.is_empty())
        || values
            .iter()
            .any(|value| !outputs.contains(value.as_str()) || !seen.insert(value))
    {
        issues.push(ValidationIssue::error(
            "router_output_invalid",
            path,
            "Router route must contain unique declared output ports",
        ));
    }
}

fn push(issues: &mut Vec<ValidationIssue>, code: &'static str, node: &str, field: &str) {
    issues.push(ValidationIssue::error(
        code,
        format!("/nodes/{node}/{field}"),
        code.replace('_', " "),
    ));
}
