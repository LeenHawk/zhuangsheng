use serde_json::Value;

use crate::{canonical, router::RouterValue};

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalJoinKey {
    pub value: Value,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinKeyError {
    pub code: &'static str,
    pub message: String,
}

pub fn canonical_join_key(value: &Value) -> Result<CanonicalJoinKey, JoinKeyError> {
    let scalar = RouterValue::from_json(value).map_err(|error| JoinKeyError {
        code: "join_key_invalid",
        message: error.message,
    })?;
    if !matches!(
        scalar,
        RouterValue::Bool(_) | RouterValue::Number(_) | RouterValue::String(_)
    ) {
        return Err(JoinKeyError {
            code: "join_key_invalid",
            message: "join key must be a non-null JSON scalar".into(),
        });
    }
    let bytes = canonical::to_vec(value).map_err(|error| JoinKeyError {
        code: "join_key_invalid",
        message: error.to_string(),
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| JoinKeyError {
        code: "join_key_invalid",
        message: error.to_string(),
    })?;
    Ok(CanonicalJoinKey { value, bytes })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn join_key_is_scalar_and_canonicalizes_equal_numbers() {
        let integer = canonical_join_key(&json!(1)).unwrap();
        let decimal: Value = serde_json::from_str("1.0").unwrap();
        assert_eq!(integer.bytes, canonical_join_key(&decimal).unwrap().bytes);
        for invalid in [Value::Null, json!([]), json!({})] {
            assert_eq!(
                canonical_join_key(&invalid).unwrap_err().code,
                "join_key_invalid"
            );
        }
    }
}
