use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::graph::{DraftNodeKind, GraphNode, RouterMatchMode};

use super::{ActivationFuel, EvaluationEnvironment, compile_expression, evaluate_expression};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterLimitReason {
    MaxVisits,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterControlSnapshot {
    pub visits: u64,
    pub first_visited_at: i64,
    pub decision_at: i64,
    pub elapsed_ms: u64,
    pub limit_reasons: Vec<RouterLimitReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterDecisionReason {
    RuleMatch,
    Default,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterDecision {
    pub dsl_version: String,
    pub matched_rule_ids: Vec<String>,
    pub evaluated_rule_ids: Vec<String>,
    pub selected_ports: Vec<String>,
    pub reason: RouterDecisionReason,
    pub control: RouterControlSnapshot,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterDecisionError {
    pub code: String,
    pub safe_message: String,
    pub rule_id: Option<String>,
    pub evaluated_rule_ids: Vec<String>,
}

pub fn evaluate_router(
    node: &GraphNode,
    inputs: &BTreeMap<String, Value>,
    memory: &Value,
    control: RouterControlSnapshot,
) -> Result<RouterDecision, RouterDecisionError> {
    let DraftNodeKind::Router {
        dsl_version,
        rules,
        match_mode,
        default_outputs,
        payload_port,
        memory: _,
        limits,
    } = &node.kind
    else {
        return Err(decision_error(
            "router_node_kind_required",
            "node is not a Router",
            None,
            Vec::new(),
        ));
    };
    let payload = select_payload(node, inputs, payload_port.as_deref())?;
    if !control.limit_reasons.is_empty() {
        let selected = limits
            .as_ref()
            .and_then(|limits| limits.on_limit_outputs.clone())
            .unwrap_or_default();
        if selected.is_empty() {
            return Err(decision_error(
                "router_control_limit_exceeded",
                "Router control limit exceeded without an on-limit route",
                None,
                Vec::new(),
            ));
        }
        return Ok(RouterDecision {
            dsl_version: dsl_version.clone(),
            matched_rule_ids: Vec::new(),
            evaluated_rule_ids: Vec::new(),
            selected_ports: selected,
            reason: RouterDecisionReason::Limit,
            control,
            payload,
        });
    }

    let inputs_value = inputs_object(node, inputs)?;
    let control_value = json!({
        "visits": control.visits,
        "elapsedMs": control.elapsed_ms,
        "limitReasons": control.limit_reasons,
    });
    let environment = EvaluationEnvironment::from_json(&inputs_value, memory, &control_value)
        .map_err(|error| decision_error(error.code, error.message, None, Vec::new()))?;
    let mut activation_fuel = ActivationFuel::default();
    let mut evaluated = Vec::new();
    let mut matched = Vec::new();
    let mut selected = Vec::new();
    let mut seen_ports = HashSet::new();
    for rule in rules {
        evaluated.push(rule.id.clone());
        let expression = compile_expression(&rule.when).map_err(|error| {
            decision_error(
                error.code,
                error.message,
                Some(rule.id.clone()),
                evaluated.clone(),
            )
        })?;
        let matches = evaluate_expression(&expression, &environment, &mut activation_fuel)
            .map_err(|error| {
                decision_error(
                    error.code,
                    error.message,
                    Some(rule.id.clone()),
                    evaluated.clone(),
                )
            })?;
        if matches {
            matched.push(rule.id.clone());
            append_unique(&mut selected, &mut seen_ports, &rule.outputs);
            if *match_mode == RouterMatchMode::First {
                break;
            }
        }
    }
    let reason = if matched.is_empty() {
        let Some(default_outputs) = default_outputs else {
            return Err(decision_error(
                "router_no_match",
                "no Router rule matched and no default route is configured",
                None,
                evaluated,
            ));
        };
        selected.clone_from(default_outputs);
        RouterDecisionReason::Default
    } else {
        RouterDecisionReason::RuleMatch
    };
    Ok(RouterDecision {
        dsl_version: dsl_version.clone(),
        matched_rule_ids: matched,
        evaluated_rule_ids: evaluated,
        selected_ports: selected,
        reason,
        control,
        payload,
    })
}

fn inputs_object(
    node: &GraphNode,
    inputs: &BTreeMap<String, Value>,
) -> Result<Value, RouterDecisionError> {
    let mut object = Map::new();
    for port in &node.inputs {
        let value = inputs.get(&port.name).ok_or_else(|| {
            decision_error(
                "router_input_missing",
                format!("resolved Router input '{}' is missing", port.name),
                None,
                Vec::new(),
            )
        })?;
        object.insert(port.name.clone(), value.clone());
    }
    Ok(Value::Object(object))
}

fn select_payload(
    node: &GraphNode,
    inputs: &BTreeMap<String, Value>,
    payload_port: Option<&str>,
) -> Result<Value, RouterDecisionError> {
    match payload_port {
        Some(port) => inputs.get(port).cloned().ok_or_else(|| {
            decision_error(
                "router_input_missing",
                format!("Router payload input '{port}' is missing"),
                None,
                Vec::new(),
            )
        }),
        None => inputs_object(node, inputs),
    }
}

fn append_unique(target: &mut Vec<String>, seen: &mut HashSet<String>, values: &[String]) {
    for value in values {
        if seen.insert(value.clone()) {
            target.push(value.clone());
        }
    }
}

fn decision_error(
    code: impl Into<String>,
    message: impl Into<String>,
    rule_id: Option<String>,
    evaluated_rule_ids: Vec<String>,
) -> RouterDecisionError {
    RouterDecisionError {
        code: code.into(),
        safe_message: message.into(),
        rule_id,
        evaluated_rule_ids,
    }
}
