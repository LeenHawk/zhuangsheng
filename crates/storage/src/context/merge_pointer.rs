use serde_json::Value;

use crate::{StorageError, StorageResult};

pub(super) fn set_pointer(target: &mut Value, path: &str, value: Value) -> StorageResult<()> {
    if path.is_empty() {
        *target = value;
        return Ok(());
    }
    let tokens: Vec<String> = path
        .split('/')
        .skip(1)
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect();
    let (last, parents) = tokens
        .split_last()
        .ok_or_else(|| StorageError::InvalidArgument("invalid merge path".into()))?;
    let mut current = target;
    for token in parents {
        current = current
            .get_mut(token)
            .ok_or_else(|| StorageError::Integrity("merge path disappeared".into()))?;
    }
    current
        .as_object_mut()
        .ok_or_else(|| StorageError::Integrity("merge path parent is not an object".into()))?
        .insert(last.clone(), value);
    Ok(())
}
