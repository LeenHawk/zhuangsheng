use serde_json::{Value, json};

use super::*;

fn environment(inputs: Value) -> EvaluationEnvironment {
    EvaluationEnvironment::from_json(
        &inputs,
        &json!({}),
        &json!({
            "visits": 1,
            "elapsedMs": 0,
            "limitReasons": []
        }),
    )
    .unwrap()
}

fn evaluate(source: &str, inputs: Value) -> Result<(bool, u64), RouterEvalError> {
    let expression = compile_expression(source).unwrap();
    let mut fuel = ActivationFuel::default();
    let result = evaluate_expression(&expression, &environment(inputs), &mut fuel)?;
    Ok((result, fuel.remaining()))
}

#[test]
fn compiler_accepts_v1_syntax_and_rejects_open_language_features() {
    compile_expression(r#"has(inputs, "score") && inputs["score"] >= 0.8 && control.visits <= 3"#)
        .unwrap();
    for source in [
        "unknown.value == 1",
        "random() == 1",
        "size(inputs, memory) == 1",
        "[inputs.value] == []",
        "inputs.value + 1 == 2",
        "inputs.value == 1 == true",
    ] {
        assert_eq!(
            compile_expression(source).unwrap_err().code,
            "router_invalid_expression",
            "source: {source}"
        );
    }
}

#[test]
fn compiler_enforces_source_list_depth_and_numeric_limits() {
    assert_eq!(
        compile_expression(&" ".repeat(4097)).unwrap_err().code,
        "router_complexity_exceeded"
    );
    let list = format!(
        "1 in [{}]",
        std::iter::repeat_n("1", 129).collect::<Vec<_>>().join(",")
    );
    assert_eq!(
        compile_expression(&list).unwrap_err().code,
        "router_complexity_exceeded"
    );
    let nested = format!("{}true{}", "!(".repeat(33), ")".repeat(33));
    assert_eq!(
        compile_expression(&nested).unwrap_err().code,
        "router_complexity_exceeded"
    );
    assert_eq!(
        compile_expression("inputs.value == 9223372036854775808")
            .unwrap_err()
            .code,
        "router_numeric_out_of_range"
    );
    assert_eq!(
        compile_expression("inputs.value == 1e19").unwrap_err().code,
        "router_numeric_out_of_range"
    );
}

#[test]
fn missing_is_distinct_from_null_and_short_circuit_is_left_to_right() {
    assert!(!evaluate("has(inputs, \"value\")", json!({})).unwrap().0);
    assert!(
        evaluate("inputs.value == null", json!({"value": null}))
            .unwrap()
            .0
    );
    assert_eq!(
        evaluate("inputs.value == null", json!({}))
            .unwrap_err()
            .code,
        "router_missing_value"
    );
    let result = evaluate("false && inputs.missing == 1", json!({})).unwrap();
    assert_eq!(result, (false, 49_998));
    assert!(
        evaluate("true || inputs.missing == 1", json!({}))
            .unwrap()
            .0
    );
}

#[test]
fn exact_numbers_compare_without_binary_float() {
    let inputs: Value = serde_json::from_str(
        r#"{"integer":1,"decimal":1.0,"tenths":0.1,"sumText":0.100000000000000001}"#,
    )
    .unwrap();
    assert!(
        evaluate("inputs.integer == inputs.decimal", inputs.clone())
            .unwrap()
            .0
    );
    assert!(
        evaluate("inputs.tenths < inputs.sumText", inputs.clone())
            .unwrap()
            .0
    );
    assert!(evaluate("inputs.integer in [0, 1.0, 2]", inputs).unwrap().0);
}

#[test]
fn functions_are_strict_and_ascii_only() {
    assert!(
        evaluate(
            r#"size(inputs.items) == 2 && contains(lower_ascii(inputs.name), "agent")"#,
            json!({"items": [1, 2], "name": "Agent角色"}),
        )
        .unwrap()
        .0
    );
    assert_eq!(
        evaluate("size(inputs.value) == 1", json!({"value": null}))
            .unwrap_err()
            .code,
        "router_type_error"
    );
    assert_eq!(
        evaluate("inputs.items[2] == 1", json!({"items": [1]}))
            .unwrap_err()
            .code,
        "router_index_out_of_range"
    );
}

#[test]
fn fixed_surcharges_and_activation_fuel_are_enforced() {
    assert_eq!(evaluate(r#""ab" == "ab""#, json!({})).unwrap().1, 49_993);
    assert_eq!(evaluate("2 in [1, 2]", json!({})).unwrap().1, 49_993);
    let long = "a".repeat(6_000);
    let error = evaluate(
        "inputs.left == inputs.right",
        json!({"left": long, "right": "a".repeat(6_000)}),
    )
    .unwrap_err();
    assert_eq!(error.code, "router_complexity_exceeded");
}
