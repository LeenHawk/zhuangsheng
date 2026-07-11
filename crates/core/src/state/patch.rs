use std::{collections::HashSet, fmt};

use serde_json::Value;

use super::{JsonPatchOp, StatePatch};

const MAX_PATCH_OPS: usize = 256;
const MAX_ID_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatePatchError {
    pub code: &'static str,
    pub message: String,
}

impl StatePatchError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for StatePatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StatePatchError {}

pub fn validate_patch(patch: &StatePatch) -> Result<(), StatePatchError> {
    for (name, value) in [
        ("aggregateId", patch.aggregate_id.as_str()),
        ("lineageKey", patch.lineage_key.as_str()),
        ("baseCommitId", patch.base_commit_id.as_str()),
        ("operationId", patch.operation_id.as_str()),
    ] {
        if value.is_empty() || value.len() > MAX_ID_BYTES {
            return Err(invalid(format!(
                "{name} must contain 1..={MAX_ID_BYTES} bytes"
            )));
        }
    }
    if patch.schema_version == 0 || patch.policy_version == 0 {
        return Err(invalid("schema and policy versions must be positive"));
    }
    if patch.ops.is_empty() || patch.ops.len() > MAX_PATCH_OPS {
        return Err(invalid(format!(
            "patch must contain 1..={MAX_PATCH_OPS} operations"
        )));
    }
    let mut append_ids = HashSet::new();
    for operation in &patch.ops {
        pointer_tokens(operation.path())?;
        if let JsonPatchOp::Append { element_id, .. } = operation
            && (element_id.is_empty()
                || element_id.len() > MAX_ID_BYTES
                || !append_ids.insert(element_id))
        {
            return Err(invalid("append elementId must be non-empty and unique"));
        }
    }
    Ok(())
}

pub fn apply_patch(base: &Value, patch: &StatePatch) -> Result<Value, StatePatchError> {
    validate_patch(patch)?;
    let mut result = base.clone();
    for operation in &patch.ops {
        apply_operation(&mut result, operation)?;
    }
    Ok(result)
}

pub fn patches_conflict(left: &StatePatch, right: &StatePatch) -> bool {
    left.ops.iter().any(|left| {
        right.ops.iter().any(|right| {
            if matches!(left, JsonPatchOp::Append { .. })
                && matches!(right, JsonPatchOp::Append { .. })
                && left.path() == right.path()
            {
                return false;
            }
            paths_overlap(left.path(), right.path())
        })
    })
}

fn apply_operation(target: &mut Value, operation: &JsonPatchOp) -> Result<(), StatePatchError> {
    let tokens = pointer_tokens(operation.path())?;
    match operation {
        JsonPatchOp::Add { value, .. } => add(target, &tokens, value.clone()),
        JsonPatchOp::Replace { value, .. } => replace(target, &tokens, value.clone()),
        JsonPatchOp::Test { value, .. } => test(target, &tokens, value),
        JsonPatchOp::Remove { .. } => remove(target, &tokens),
        JsonPatchOp::Append { value, .. } => append(target, &tokens, value.clone()),
    }
}

fn add(target: &mut Value, tokens: &[String], value: Value) -> Result<(), StatePatchError> {
    if tokens.is_empty() {
        *target = value;
        return Ok(());
    }
    let (parent, key) = parent_mut(target, tokens)?;
    match parent {
        Value::Object(object) => {
            object.insert(key.into(), value);
            Ok(())
        }
        Value::Array(array) if key == "-" => {
            array.push(value);
            Ok(())
        }
        Value::Array(array) => {
            let index = array_index(key, true, array.len())?;
            array.insert(index, value);
            Ok(())
        }
        _ => Err(type_error("add parent must be an object or array")),
    }
}

fn replace(target: &mut Value, tokens: &[String], value: Value) -> Result<(), StatePatchError> {
    if tokens.is_empty() {
        *target = value;
        return Ok(());
    }
    let slot = locate_mut(target, tokens)?;
    *slot = value;
    Ok(())
}

fn test(target: &mut Value, tokens: &[String], expected: &Value) -> Result<(), StatePatchError> {
    let actual = locate(target, tokens)?;
    if actual == expected {
        Ok(())
    } else {
        Err(StatePatchError::new(
            "state_test_failed",
            "StatePatch test operation failed",
        ))
    }
}

fn remove(target: &mut Value, tokens: &[String]) -> Result<(), StatePatchError> {
    if tokens.is_empty() {
        return Err(invalid("root value cannot be removed"));
    }
    let (parent, key) = parent_mut(target, tokens)?;
    match parent {
        Value::Object(object) => object
            .remove(key)
            .map(|_| ())
            .ok_or_else(|| missing("remove target does not exist")),
        Value::Array(array) => {
            let index = array_index(key, false, array.len())?;
            array.remove(index);
            Ok(())
        }
        _ => Err(type_error("remove parent must be an object or array")),
    }
}

fn append(target: &mut Value, tokens: &[String], value: Value) -> Result<(), StatePatchError> {
    match locate_mut(target, tokens)? {
        Value::Array(array) => {
            array.push(value);
            Ok(())
        }
        _ => Err(type_error("append target must be an array")),
    }
}

fn locate<'a>(target: &'a Value, tokens: &[String]) -> Result<&'a Value, StatePatchError> {
    let mut current = target;
    for token in tokens {
        current = match current {
            Value::Object(object) => object
                .get(token)
                .ok_or_else(|| missing("object member does not exist"))?,
            Value::Array(array) => array
                .get(array_index(token, false, array.len())?)
                .ok_or_else(|| missing("array member does not exist"))?,
            _ => return Err(type_error("JSON Pointer traverses a scalar value")),
        };
    }
    Ok(current)
}

fn locate_mut<'a>(
    target: &'a mut Value,
    tokens: &[String],
) -> Result<&'a mut Value, StatePatchError> {
    if tokens.is_empty() {
        return Ok(target);
    }
    let (first, rest) = tokens.split_first().expect("non-empty tokens");
    let child = match target {
        Value::Object(object) => object
            .get_mut(first)
            .ok_or_else(|| missing("object member does not exist"))?,
        Value::Array(array) => {
            let index = array_index(first, false, array.len())?;
            array
                .get_mut(index)
                .ok_or_else(|| missing("array member does not exist"))?
        }
        _ => return Err(type_error("JSON Pointer traverses a scalar value")),
    };
    locate_mut(child, rest)
}

fn parent_mut<'a>(
    target: &'a mut Value,
    tokens: &'a [String],
) -> Result<(&'a mut Value, &'a str), StatePatchError> {
    let (key, parents) = tokens
        .split_last()
        .ok_or_else(|| invalid("operation requires a non-root path"))?;
    Ok((locate_mut(target, parents)?, key))
}

fn pointer_tokens(pointer: &str) -> Result<Vec<String>, StatePatchError> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    if !pointer.starts_with('/') {
        return Err(invalid("JSON Pointer must be empty or start with '/'"));
    }
    pointer[1..]
        .split('/')
        .map(|token| {
            let mut decoded = String::new();
            let mut chars = token.chars();
            while let Some(character) = chars.next() {
                if character != '~' {
                    decoded.push(character);
                    continue;
                }
                decoded.push(match chars.next() {
                    Some('0') => '~',
                    Some('1') => '/',
                    _ => return Err(invalid("JSON Pointer contains an invalid escape")),
                });
            }
            Ok(decoded)
        })
        .collect()
}

fn array_index(token: &str, allow_end: bool, length: usize) -> Result<usize, StatePatchError> {
    if token.is_empty() || token.len() > 1 && token.starts_with('0') {
        return Err(invalid("array index is not canonical"));
    }
    let index = token
        .parse::<usize>()
        .map_err(|_| invalid("array index is invalid"))?;
    if index < length || allow_end && index == length {
        Ok(index)
    } else {
        Err(missing("array index is out of range"))
    }
}

fn paths_overlap(left: &str, right: &str) -> bool {
    let left = pointer_tokens(left).unwrap_or_default();
    let right = pointer_tokens(right).unwrap_or_default();
    left.iter().zip(&right).all(|(left, right)| left == right)
}

fn invalid(message: impl Into<String>) -> StatePatchError {
    StatePatchError::new("invalid_state_patch", message)
}

fn missing(message: impl Into<String>) -> StatePatchError {
    StatePatchError::new("state_path_missing", message)
}

fn type_error(message: impl Into<String>) -> StatePatchError {
    StatePatchError::new("state_type_error", message)
}
