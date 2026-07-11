use std::collections::{HashMap, HashSet, VecDeque};

use crate::{DomainError, DomainResult, ValidationIssue, canonical, schema};

use super::{
    AppliedGraph, AppliedGraphDefinition, DraftGraphEdge, DraftGraphNode, DraftNodeKind,
    GraphDraft, GraphEdge, GraphNode, InputPortDefinition, OutputPortDefinition, RunLimits,
    SchemaSemanticDigest, cycle::validate_cycles,
};

pub fn apply_graph(draft: GraphDraft, taxonomy: u32, decoder: u32) -> DomainResult<AppliedGraph> {
    let mut issues = Vec::new();
    if taxonomy == 0 || decoder == 0 {
        issues.push(issue("operation_version_not_positive", "/"));
    }
    let nodes: Vec<_> = draft
        .nodes
        .into_iter()
        .map(|node| normalize_node(node, &mut issues))
        .collect();
    validate_nodes(&nodes, &mut issues);
    let edges = normalize_edges(draft.edges, &mut issues)?;
    validate_edges(&nodes, &edges, &mut issues);
    validate_outputs(&nodes, &draft.output_contract, &mut issues);
    validate_cycles(&nodes, &edges, &mut issues);
    let mut warnings = unreachable_warnings(&nodes, &edges);
    let limits = normalize_limits(draft.limits, &mut issues);
    let schemas = compile_schemas(
        &nodes,
        draft.run_input_schema.as_ref(),
        &draft.output_contract,
        &mut issues,
    );
    if !issues.is_empty() {
        return Err(DomainError::GraphValidation(issues));
    }
    let mut schema_semantics: Vec<_> = schemas
        .iter()
        .map(|compiled| SchemaSemanticDigest {
            canonical_document_hash: compiled.canonical_document_hash.clone(),
            schema_hash: compiled.schema_hash.clone(),
            compiler_id: compiled.compiler_id.clone(),
            compiler_version: compiled.compiler_version.clone(),
            payload_format_version: compiled.payload_format_version,
            compiled_payload_hash: compiled.compiled_payload_hash.clone(),
        })
        .collect();
    schema_semantics.sort_by(|left, right| left.schema_hash.cmp(&right.schema_hash));
    warnings.sort_by(|left, right| left.path.cmp(&right.path));
    let definition = AppliedGraphDefinition {
        schema_version: 1,
        graph_id: draft.graph_id,
        operation_taxonomy_version: taxonomy,
        adapter_decoder_version: decoder,
        nodes,
        edges,
        run_input_schema: draft.run_input_schema,
        output_contract: draft.output_contract,
        limits,
        schema_semantics,
    };
    let content_hash = canonical::hash(&definition)?;
    Ok(AppliedGraph {
        definition,
        content_hash,
        schemas,
        warnings,
    })
}

fn normalize_node(node: DraftGraphNode, issues: &mut Vec<ValidationIssue>) -> GraphNode {
    let mut inputs = node.inputs;
    let mut outputs = node.outputs;
    let is_input = matches!(node.kind, DraftNodeKind::Input { .. });
    let is_output = matches!(node.kind, DraftNodeKind::Output { .. });
    if is_input {
        if !inputs.is_empty() {
            issues.push(issue(
                "input_node_has_inputs",
                format!("/nodes/{}/inputs", node.id),
            ));
        }
        inputs.clear();
        if outputs.is_empty() {
            outputs.push(default_output());
        }
    } else if is_output {
        if inputs.is_empty() {
            inputs.push(default_input());
        }
        if !outputs.is_empty() {
            issues.push(issue(
                "output_node_has_outputs",
                format!("/nodes/{}/outputs", node.id),
            ));
        }
        outputs.clear();
    } else {
        if inputs.is_empty() {
            inputs.push(default_input());
        }
        if outputs.is_empty() {
            issues.push(issue(
                "router_outputs_required",
                format!("/nodes/{}/outputs", node.id),
            ));
        }
    }
    if let Some(declared) = node.is_entry
        && declared != is_input
    {
        issues.push(issue(
            "node_entry_kind_mismatch",
            format!("/nodes/{}/isEntry", node.id),
        ));
    }
    if is_input && outputs.len() != 1 {
        issues.push(issue(
            "input_node_output_count",
            format!("/nodes/{}/outputs", node.id),
        ));
    }
    if is_output && inputs.len() != 1 {
        issues.push(issue(
            "output_node_input_count",
            format!("/nodes/{}/inputs", node.id),
        ));
    }
    GraphNode {
        id: node.id,
        name: node.name,
        is_entry: is_input,
        inputs,
        outputs,
        timeout_ms: node.timeout_ms,
        kind: node.kind,
    }
}

fn validate_nodes(nodes: &[GraphNode], issues: &mut Vec<ValidationIssue>) {
    let mut ids = HashSet::new();
    let mut entries = 0;
    for node in nodes {
        if node.id.is_empty() || !ids.insert(&node.id) {
            issues.push(issue("duplicate_or_empty_node_id", "/nodes"));
        }
        if node.is_entry {
            entries += 1;
        }
        unique_ports(node, issues);
        if let DraftNodeKind::Router {
            dsl_version,
            rules,
            default_outputs,
            payload_port,
            limits,
            ..
        } = &node.kind
        {
            if dsl_version != "router-dsl-v1" {
                issues.push(issue(
                    "unsupported_router_dsl",
                    format!("/nodes/{}/dslVersion", node.id),
                ));
            }
            let outputs: HashSet<_> = node.outputs.iter().map(|port| port.name.as_str()).collect();
            let mut rule_ids = HashSet::new();
            for rule in rules {
                if rule.id.is_empty() || !rule_ids.insert(&rule.id) {
                    issues.push(issue(
                        "duplicate_router_rule",
                        format!("/nodes/{}/rules", node.id),
                    ));
                }
                check_output_names(&node.id, &rule.outputs, &outputs, issues);
            }
            check_output_names(&node.id, default_outputs, &outputs, issues);
            if let Some(port) = payload_port
                && !node.inputs.iter().any(|input| input.name == *port)
            {
                issues.push(issue(
                    "router_payload_port_missing",
                    format!("/nodes/{}/payloadPort", node.id),
                ));
            }
            if let Some(limits) = limits {
                check_output_names(&node.id, &limits.on_limit_outputs, &outputs, issues);
            }
        }
    }
    if entries == 0 {
        issues.push(issue("graph_has_no_input_node", "/nodes"));
    }
}

fn normalize_edges(
    edges: Vec<DraftGraphEdge>,
    issues: &mut Vec<ValidationIssue>,
) -> DomainResult<Vec<GraphEdge>> {
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for edge in edges {
        let id = edge.id.unwrap_or(format!(
            "edge_{}",
            &canonical::hash(&(&edge.from, &edge.to))?[7..31]
        ));
        if id.is_empty() || !ids.insert(id.clone()) {
            issues.push(issue("duplicate_or_empty_edge_id", "/edges"));
        }
        result.push(GraphEdge {
            id,
            from: edge.from,
            to: edge.to,
        });
    }
    result.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(result)
}

fn validate_edges(nodes: &[GraphNode], edges: &[GraphEdge], issues: &mut Vec<ValidationIssue>) {
    let map: HashMap<_, _> = nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let mut incoming = HashSet::new();
    for edge in edges {
        let source = map.get(edge.from.node_id.as_str());
        let target = map.get(edge.to.node_id.as_str());
        if !source.is_some_and(|node| {
            node.outputs
                .iter()
                .any(|port| port.name == edge.from.output)
        }) {
            issues.push(issue("edge_source_missing", format!("/edges/{}", edge.id)));
        }
        if !target.is_some_and(|node| node.inputs.iter().any(|port| port.name == edge.to.input)) {
            issues.push(issue("edge_target_missing", format!("/edges/{}", edge.id)));
        }
        if !incoming.insert((&edge.to.node_id, &edge.to.input)) {
            issues.push(issue(
                "input_has_multiple_edges",
                format!("/edges/{}", edge.id),
            ));
        }
    }
    for node in nodes.iter().filter(|node| !node.is_entry) {
        for input in &node.inputs {
            if !incoming.contains(&(&node.id, &input.name)) {
                issues.push(issue(
                    "required_input_unconnected",
                    format!("/nodes/{}/inputs/{}", node.id, input.name),
                ));
            }
        }
    }
}

fn validate_outputs(
    nodes: &[GraphNode],
    contracts: &[super::GraphOutputContractEntry],
    issues: &mut Vec<ValidationIssue>,
) {
    let mut keys = HashSet::new();
    for contract in contracts {
        if contract.key.is_empty() || !keys.insert(&contract.key) {
            issues.push(issue("duplicate_or_empty_output_key", "/outputContract"));
        }
    }
    for node in nodes {
        if let DraftNodeKind::Output { output_key } = &node.kind {
            if !contracts.iter().any(|contract| contract.key == *output_key) {
                issues.push(issue(
                    "output_node_contract_missing",
                    format!("/nodes/{}/outputKey", node.id),
                ));
            }
        }
    }
    for contract in contracts {
        let count = nodes.iter().filter(|node| matches!(&node.kind, DraftNodeKind::Output { output_key } if output_key == &contract.key)).count();
        if count != 1 {
            issues.push(issue(
                "output_contract_sink_count",
                format!("/outputContract/{}", contract.key),
            ));
        }
    }
}

fn compile_schemas(
    nodes: &[GraphNode],
    run_input: Option<&schema::JsonSchemaSpec>,
    contracts: &[super::GraphOutputContractEntry],
    issues: &mut Vec<ValidationIssue>,
) -> Vec<schema::SchemaCompilationDraft> {
    let mut specs = Vec::new();
    if let Some(spec) = run_input {
        specs.push(spec);
    }
    for node in nodes {
        for port in &node.inputs {
            if let Some(spec) = &port.schema {
                specs.push(spec);
            }
        }
        for port in &node.outputs {
            if let Some(spec) = &port.schema {
                specs.push(spec);
            }
        }
    }
    for contract in contracts {
        if let Some(spec) = &contract.schema {
            specs.push(spec);
        }
    }
    let mut seen = HashSet::new();
    specs
        .into_iter()
        .filter_map(|spec| match schema::compile(spec) {
            Ok(compiled) if seen.insert(compiled.schema_hash.clone()) => Some(compiled),
            Ok(_) => None,
            Err(DomainError::SchemaValidation(found)) => {
                issues.extend(found);
                None
            }
            Err(error) => {
                issues.push(ValidationIssue::error(
                    "schema_compile_failed",
                    "/",
                    error.to_string(),
                ));
                None
            }
        })
        .collect()
}

fn normalize_limits(
    draft: Option<super::DraftRunLimits>,
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
        issues.push(issue("run_limit_not_positive", "/limits"));
    }
    limits
}

fn unreachable_warnings(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<ValidationIssue> {
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

fn unique_ports(node: &GraphNode, issues: &mut Vec<ValidationIssue>) {
    let mut names = HashSet::new();
    for name in node
        .inputs
        .iter()
        .map(|p| &p.name)
        .chain(node.outputs.iter().map(|p| &p.name))
    {
        if name.is_empty() || !names.insert(name) {
            issues.push(issue(
                "duplicate_or_empty_port",
                format!("/nodes/{}/ports", node.id),
            ));
        }
    }
}
fn check_output_names(
    node: &str,
    values: &[String],
    outputs: &HashSet<&str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut seen = HashSet::new();
    for value in values {
        if !outputs.contains(value.as_str()) || !seen.insert(value) {
            issues.push(issue(
                "router_output_invalid",
                format!("/nodes/{node}/outputs"),
            ));
        }
    }
}
fn default_input() -> InputPortDefinition {
    InputPortDefinition {
        name: "default".into(),
        schema: None,
        binding: Default::default(),
    }
}
fn default_output() -> OutputPortDefinition {
    OutputPortDefinition {
        name: "default".into(),
        schema: None,
    }
}
fn issue(code: &str, path: impl Into<String>) -> ValidationIssue {
    ValidationIssue::error(code, path, code.replace('_', " "))
}
