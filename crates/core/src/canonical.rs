use std::fmt::Write;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{DomainError, DomainResult};

pub const MAX_JSON_DEPTH: usize = 128;
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_NUMBER_DIGITS: usize = 128;
pub const MAX_EXPONENT_MAGNITUDE: i32 = 1024;

pub fn parse(input: &str) -> DomainResult<Value> {
    if input.len() > MAX_JSON_BYTES {
        return Err(DomainError::JsonLimit("max bytes exceeded".into()));
    }
    let value: Value =
        serde_json::from_str(input).map_err(|error| DomainError::InvalidJson(error.to_string()))?;
    validate_limits(&value, 0)?;
    Ok(value)
}

pub fn to_vec<T: Serialize>(value: &T) -> DomainResult<Vec<u8>> {
    let value = serde_json::to_value(value)
        .map_err(|error| DomainError::Serialization(error.to_string()))?;
    validate_limits(&value, 0)?;
    let mut output = String::new();
    write_value(&value, &mut output)?;
    Ok(output.into_bytes())
}

pub fn to_string<T: Serialize>(value: &T) -> DomainResult<String> {
    String::from_utf8(to_vec(value)?).map_err(|error| DomainError::Serialization(error.to_string()))
}

pub fn hash<T: Serialize>(value: &T) -> DomainResult<String> {
    Ok(hash_bytes(&to_vec(value)?))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn validate_limits(value: &Value, depth: usize) -> DomainResult<()> {
    if depth > MAX_JSON_DEPTH {
        return Err(DomainError::JsonLimit("max depth exceeded".into()));
    }
    match value {
        Value::Number(number) => {
            normalize_number(&number.to_string())?;
        }
        Value::Array(values) => {
            for value in values {
                validate_limits(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_limits(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn write_value(value: &Value, output: &mut String) -> DomainResult<()> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&normalize_number(&value.to_string())?),
        Value::String(value) => output.push_str(
            &serde_json::to_string(value)
                .map_err(|error| DomainError::Serialization(error.to_string()))?,
        ),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_value(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(
                    &serde_json::to_string(key)
                        .map_err(|error| DomainError::Serialization(error.to_string()))?,
                );
                output.push(':');
                write_value(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn normalize_number(raw: &str) -> DomainResult<String> {
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map_or((false, raw), |value| (true, value));
    let exponent_index = unsigned.find(['e', 'E']);
    let (coefficient, exponent) = exponent_index.map_or((unsigned, 0_i32), |index| {
        let exponent = unsigned[index + 1..].parse::<i32>().unwrap_or(i32::MAX);
        (&unsigned[..index], exponent)
    });
    if exponent.abs() > MAX_EXPONENT_MAGNITUDE {
        return Err(DomainError::JsonLimit("number exponent exceeded".into()));
    }
    let (integer, fraction) = coefficient.split_once('.').unwrap_or((coefficient, ""));
    let integer = integer.trim_start_matches('0');
    let fraction = fraction.trim_end_matches('0');
    let digits = format!("{integer}{fraction}");
    let significant = digits.trim_start_matches('0');
    if significant.len() > MAX_NUMBER_DIGITS {
        return Err(DomainError::JsonLimit("number digits exceeded".into()));
    }
    if significant.is_empty() {
        return Ok("0".into());
    }
    let decimal_position = integer.len() as i32 + exponent;
    let mut result = String::new();
    if negative {
        result.push('-');
    }
    if decimal_position <= 0 {
        result.push_str("0.");
        for _ in 0..-decimal_position {
            result.push('0');
        }
        result.push_str(&digits);
    } else if decimal_position as usize >= digits.len() {
        result.push_str(&digits);
        for _ in 0..decimal_position as usize - digits.len() {
            result.push('0');
        }
    } else {
        let split = decimal_position as usize;
        write!(result, "{}.{}", &digits[..split], &digits[split..]).unwrap();
    }
    if result.contains('.') {
        while result.ends_with('0') {
            result.pop();
        }
        if result.ends_with('.') {
            result.pop();
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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
}
