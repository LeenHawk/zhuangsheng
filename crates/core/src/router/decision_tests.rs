use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::graph::{
    DraftNodeKind, GraphNode, InputPortDefinition, OutputPortDefinition, RouterLimits,
    RouterMatchMode, RouterRule,
};

use super::*;

fn node(match_mode: RouterMatchMode, payload_port: Option<&str>) -> GraphNode {
    GraphNode {
        id: "router".into(),
        name: None,
        is_entry: false,
        inputs: vec![port_in("score"), port_in("payload")],
        outputs: ["retry", "done", "shared", "limit"]
            .into_iter()
            .map(port_out)
            .collect(),
        timeout_ms: None,
        retry_policy: None,
        kind: DraftNodeKind::Router {
            dsl_version: "router-dsl-v1".into(),
            rules: vec![
                RouterRule {
                    id: "low".into(),
                    when: "inputs.score < 0.8".into(),
                    outputs: vec!["retry".into(), "shared".into()],
                },
                RouterRule {
                    id: "positive".into(),
                    when: "inputs.score > 0".into(),
                    outputs: vec!["shared".into(), "done".into()],
                },
            ],
            match_mode,
            default_outputs: Some(vec!["done".into()]),
            payload_port: payload_port.map(String::from),
            memory: None,
            limits: Some(RouterLimits {
                max_visits_per_run: Some(3),
                timeout_ms_per_run: None,
                max_read_reconciles: Some(2),
                on_limit_outputs: Some(vec!["limit".into()]),
            }),
        },
    }
}

fn inputs(score: Value) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("score".into(), score),
        ("payload".into(), json!({"message": "hello"})),
    ])
}

fn control(reasons: Vec<RouterLimitReason>) -> RouterControlSnapshot {
    RouterControlSnapshot {
        visits: 1,
        first_visited_at: 100,
        decision_at: 100,
        elapsed_ms: 0,
        limit_reasons: reasons,
    }
}

#[test]
fn first_and_all_preserve_rule_and_port_order() {
    let first = evaluate_router(
        &node(RouterMatchMode::First, Some("payload")),
        &inputs(json!(0.5)),
        &json!({}),
        control(Vec::new()),
    )
    .unwrap();
    assert_eq!(first.matched_rule_ids, ["low"]);
    assert_eq!(first.evaluated_rule_ids, ["low"]);
    assert_eq!(first.selected_ports, ["retry", "shared"]);
    assert_eq!(first.payload, json!({"message": "hello"}));

    let all = evaluate_router(
        &node(RouterMatchMode::All, None),
        &inputs(json!(0.5)),
        &json!({}),
        control(Vec::new()),
    )
    .unwrap();
    assert_eq!(all.matched_rule_ids, ["low", "positive"]);
    assert_eq!(all.selected_ports, ["retry", "shared", "done"]);
    assert_eq!(all.payload["score"], json!(0.5));
}

#[test]
fn default_and_limit_skip_unneeded_routes() {
    let mut default_node = node(RouterMatchMode::First, None);
    let DraftNodeKind::Router { rules, .. } = &mut default_node.kind else {
        unreachable!()
    };
    rules[0].when = "inputs.score < 0".into();
    rules[1].when = "inputs.score > 1".into();
    let default = evaluate_router(
        &default_node,
        &inputs(json!(0.5)),
        &json!({}),
        control(Vec::new()),
    )
    .unwrap();
    assert_eq!(default.reason, RouterDecisionReason::Default);
    assert_eq!(default.evaluated_rule_ids, ["low", "positive"]);
    assert_eq!(default.selected_ports, ["done"]);

    let limit = evaluate_router(
        &node(RouterMatchMode::All, None),
        &inputs(json!(0.5)),
        &json!({}),
        control(vec![RouterLimitReason::MaxVisits]),
    )
    .unwrap();
    assert_eq!(limit.reason, RouterDecisionReason::Limit);
    assert!(limit.evaluated_rule_ids.is_empty());
    assert_eq!(limit.selected_ports, ["limit"]);
}

#[test]
fn evaluation_error_stops_at_current_rule_without_default() {
    let mut router = node(RouterMatchMode::All, None);
    let DraftNodeKind::Router { rules, .. } = &mut router.kind else {
        unreachable!()
    };
    rules[0].when = "inputs.unknown == true".into();
    let error =
        evaluate_router(&router, &inputs(json!(1)), &json!({}), control(Vec::new())).unwrap_err();
    assert_eq!(error.code, "router_missing_value");
    assert_eq!(error.rule_id.as_deref(), Some("low"));
    assert_eq!(error.evaluated_rule_ids, ["low"]);
}

fn port_in(name: &str) -> InputPortDefinition {
    InputPortDefinition {
        name: name.into(),
        schema: None,
        binding: Default::default(),
    }
}

fn port_out(name: &str) -> OutputPortDefinition {
    OutputPortDefinition {
        name: name.into(),
        schema: None,
    }
}
