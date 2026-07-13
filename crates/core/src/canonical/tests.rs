use serde::{Deserialize, Serialize};
use serde_json::{Number, Value, json};

use super::*;

#[test]
fn sorts_object_keys_and_normalizes_numbers() {
    let value = parse(r#"{"z":1.00e2,"a":{"b":-0.000,"a":2}}"#).unwrap();
    assert_eq!(to_string(&value).unwrap(), r#"{"a":{"a":2,"b":0},"z":100}"#);
}

#[test]
fn equal_values_have_equal_hashes() {
    let left = parse(r#"{"b":1e1,"a":true}"#).unwrap();
    let right = json!({"a": true, "b": 10});
    assert_eq!(hash(&left).unwrap(), hash(&right).unwrap());
}

#[test]
fn rejects_duplicate_keys_and_number_overflow() {
    assert!(matches!(
        parse(r#"{"a":1,"a":1}"#),
        Err(DomainError::InvalidJson(_))
    ));
    assert!(matches!(
        parse(&format!("{{\"n\":{}}}", "1".repeat(129))),
        Err(DomainError::JsonLimit(_))
    ));
    assert!(matches!(
        parse(r#"{"n":1e1025}"#),
        Err(DomainError::JsonLimit(_))
    ));
    assert!(matches!(
        parse(r#"{"n":10e1024}"#),
        Err(DomainError::JsonLimit(_))
    ));
    assert!(parse(r#"{"n":0e1024}"#).is_ok());
}

#[test]
fn exact_decimal_vectors_survive_canonical_and_typed_roundtrips() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        schema_version: u64,
        value: Value,
    }

    let value = parse(
        r#"{"unsafeInteger":9007199254740993,"decimal":1.2345678901234567890123456789,"exponent":12345678901234567890e-17}"#,
    ).unwrap();
    let source = to_string(&Envelope {
        schema_version: 1,
        value,
    })
    .unwrap();
    assert!(source.contains("9007199254740993"));
    assert!(source.contains("1.2345678901234567890123456789"));
    assert!(source.contains("123.4567890123456789"));
    let decoded: Envelope = serde_json::from_str(&source).unwrap();
    assert_eq!(decoded.schema_version, 1);
    assert_eq!(to_string(&decoded).unwrap(), source);
}

#[test]
fn typed_floats_survive_canonical_roundtrips() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Envelope {
        temperature: f64,
        optional: Option<f64>,
    }

    let source = to_string(&Envelope {
        temperature: 0.8,
        optional: Some(0.2),
    })
    .unwrap();
    assert_eq!(source, r#"{"optional":0.2,"temperature":0.8}"#);
    assert_eq!(
        serde_json::from_str::<Envelope>(&source).unwrap(),
        Envelope {
            temperature: 0.8,
            optional: Some(0.2)
        }
    );
}

#[test]
fn generation_floats_survive_flattened_graph_roundtrips() {
    use crate::graph::{GenerationOptionsIr, LlmRequestOptions};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Node {
        Llm { request: LlmRequestOptions },
    }

    let node = Node::Llm {
        request: LlmRequestOptions {
            generation: Some(GenerationOptionsIr {
                temperature: Some(0.8),
                top_p: Some(0.9),
                max_output_tokens: Some(512),
                stop: Vec::new(),
                seed: None,
            }),
            ..Default::default()
        },
    };
    let source = to_string(&node).unwrap();
    assert_eq!(serde_json::from_str::<Node>(&source).unwrap(), node);
}

#[test]
fn enforces_structural_and_expanded_output_limits() {
    assert!(matches!(
        to_vec(&"x".repeat(MAX_STRING_BYTES + 1)),
        Err(DomainError::JsonLimit(_))
    ));
    assert!(matches!(
        to_vec(&vec![Value::Null; MAX_COLLECTION_ITEMS + 1]),
        Err(DomainError::JsonLimit(_))
    ));
    let expanded = Value::Number(Number::from_string_unchecked(format!(
        "1e{}",
        MAX_EXPONENT_MAGNITUDE
    )));
    let count = MAX_JSON_BYTES / (MAX_EXPONENT_MAGNITUDE as usize + 2) + 1;
    assert!(matches!(
        to_vec(&vec![expanded; count]),
        Err(DomainError::JsonLimit(_))
    ));
}
