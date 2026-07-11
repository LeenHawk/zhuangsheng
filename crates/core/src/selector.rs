use serde_json::Value;
use serde_json_path::JsonPath;

use crate::graph::{InputSelector, SelectorResult};

pub fn validate(selector: &InputSelector) -> Result<(), String> {
    match selector {
        InputSelector::WholeValue => Ok(()),
        InputSelector::JsonPointer { pointer } => validate_pointer(pointer),
        InputSelector::JsonPath { path, .. } => JsonPath::parse(path)
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

pub fn select(selector: &InputSelector, value: &Value, max_matches: u64) -> Result<Value, String> {
    validate(selector)?;
    match selector {
        InputSelector::WholeValue => Ok(value.clone()),
        InputSelector::JsonPointer { pointer } => value
            .pointer(pointer)
            .cloned()
            .ok_or_else(|| format!("JSON Pointer did not match: {pointer}")),
        InputSelector::JsonPath { path, result } => {
            let path = JsonPath::parse(path).map_err(|error| error.to_string())?;
            let matches = path.query(value).all();
            if matches.len() as u64 > max_matches {
                return Err("JSONPath match limit exceeded".into());
            }
            match result {
                SelectorResult::One if matches.len() == 1 => Ok(matches[0].clone()),
                SelectorResult::One => Err(format!(
                    "JSONPath expected one match but found {}",
                    matches.len()
                )),
                SelectorResult::Many => Ok(Value::Array(matches.into_iter().cloned().collect())),
            }
        }
    }
}

fn validate_pointer(pointer: &str) -> Result<(), String> {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err("JSON Pointer must be empty or start with '/'".into());
    }
    let bytes = pointer.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'~' {
            if !matches!(bytes.get(index + 1), Some(b'0' | b'1')) {
                return Err("JSON Pointer contains an invalid escape".into());
            }
            index += 1;
        }
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn selects_pointer_and_rfc_json_path() {
        let value = json!({"items":[{"name":"a"},{"name":"b"}]});
        assert_eq!(
            select(
                &InputSelector::JsonPointer {
                    pointer: "/items/0/name".into()
                },
                &value,
                10
            )
            .unwrap(),
            json!("a")
        );
        assert_eq!(
            select(
                &InputSelector::JsonPath {
                    path: "$.items[*].name".into(),
                    result: SelectorResult::Many
                },
                &value,
                10
            )
            .unwrap(),
            json!(["a", "b"])
        );
    }

    #[test]
    fn rejects_invalid_pointer_and_wrong_cardinality() {
        assert!(
            validate(&InputSelector::JsonPointer {
                pointer: "a".into()
            })
            .is_err()
        );
        assert!(
            select(
                &InputSelector::JsonPath {
                    path: "$[*]".into(),
                    result: SelectorResult::One
                },
                &json!([1, 2]),
                10
            )
            .is_err()
        );
    }
}
