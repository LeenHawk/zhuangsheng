use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{DomainError, DomainResult};

pub const MAX_JSON_DEPTH: usize = 128;
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_NUMBER_DIGITS: usize = 128;
pub const MAX_EXPONENT_MAGNITUDE: i32 = 1024;
pub const MAX_COLLECTION_ITEMS: usize = 100_000;
pub const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;

mod number;
mod parse;

use number::normalize_number;

pub(crate) fn validate_number(raw: &str, max_digits: usize, max_exponent: i64) -> DomainResult<()> {
    number::validate_number(raw, max_digits, max_exponent)
}

pub fn parse(input: &str) -> DomainResult<Value> {
    if input.len() > MAX_JSON_BYTES {
        return Err(DomainError::JsonLimit("max bytes exceeded".into()));
    }
    parse::reject_duplicate_keys(input)?;
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
    if output.len() > MAX_JSON_BYTES {
        return Err(DomainError::JsonLimit("max bytes exceeded".into()));
    }
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
        Value::String(value) if value.len() > MAX_STRING_BYTES => {
            return Err(DomainError::JsonLimit("string bytes exceeded".into()));
        }
        Value::Number(number) => {
            normalize_number(&number.to_string())?;
        }
        Value::Array(values) => {
            if values.len() > MAX_COLLECTION_ITEMS {
                return Err(DomainError::JsonLimit("collection items exceeded".into()));
            }
            for value in values {
                validate_limits(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            if values.len() > MAX_COLLECTION_ITEMS {
                return Err(DomainError::JsonLimit("collection items exceeded".into()));
            }
            for (key, value) in values {
                if key.len() > MAX_STRING_BYTES {
                    return Err(DomainError::JsonLimit("string bytes exceeded".into()));
                }
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

#[cfg(test)]
mod tests;
