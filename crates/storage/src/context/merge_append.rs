use std::collections::BTreeMap;

use zhuangsheng_core::context_merge::{
    MergeAppendItem, ThreeWayMergeAnalysis, merge_append_only_arrays,
};

use crate::{StorageError, StorageResult};

use super::merge_append_history::AppendHistory;

pub(super) struct VerifiedAppendMerge {
    pub entries: Vec<MergeAppendItem>,
    pub blocked_paths: Vec<String>,
}

pub(super) fn apply_verified_appends(
    analysis: &mut ThreeWayMergeAnalysis,
    base: &serde_json::Value,
    source: &serde_json::Value,
    target: &serde_json::Value,
    source_history: &AppendHistory,
    target_history: &AppendHistory,
) -> StorageResult<VerifiedAppendMerge> {
    validate_cross_branch_identities(source_history, target_history)?;
    let mut remaining = Vec::new();
    let mut integrated = Vec::new();
    for conflict in analysis.conflicts.drain(..) {
        let path = conflict.path.as_str();
        let verified = if source_history.is_append_only(path) && target_history.is_append_only(path)
        {
            merge_append_only_arrays(
                path,
                base.pointer(path).unwrap_or(&serde_json::Value::Null),
                source.pointer(path).unwrap_or(&serde_json::Value::Null),
                target.pointer(path).unwrap_or(&serde_json::Value::Null),
                source_history.items(path),
                target_history.items(path),
            )
        } else {
            None
        };
        if let Some(merged) = verified {
            super::merge_pointer::set_pointer(&mut analysis.merged, path, merged.value)?;
            integrated.extend(merged.integrated_source);
        } else {
            remaining.push(conflict);
        }
    }
    analysis.conflicts = remaining;
    for path in source_history.append_paths() {
        let Some(base_value) = base.pointer(path) else {
            continue;
        };
        let Some(source_value) = source.pointer(path) else {
            continue;
        };
        let Some(target_value) = target.pointer(path) else {
            continue;
        };
        let already_integrated = integrated.iter().any(|item| item.path == path);
        if already_integrated || !source_history.is_append_only(path) {
            continue;
        }
        let one_sided = target_value == base_value;
        let equal_change = source_value == target_value;
        if !one_sided && !equal_change {
            continue;
        }
        let Some(verified) = merge_append_only_arrays(
            path,
            base_value,
            source_value,
            target_value,
            source_history.items(path),
            target_history.items(path),
        ) else {
            continue;
        };
        if equal_change || verified.value == *analysis.merged.pointer(path).unwrap_or(target_value)
        {
            integrated.extend(verified.integrated_source);
        }
    }
    let mut blocked_paths: Vec<_> = source_history
        .blocked_paths()
        .chain(target_history.blocked_paths())
        .map(String::from)
        .collect();
    blocked_paths.sort();
    blocked_paths.dedup();
    integrated.sort_by(|left, right| left.element_id.cmp(&right.element_id));
    integrated.dedup_by(|left, right| left.element_id == right.element_id);
    Ok(VerifiedAppendMerge {
        entries: integrated,
        blocked_paths,
    })
}

fn validate_cross_branch_identities(
    source: &AppendHistory,
    target: &AppendHistory,
) -> StorageResult<()> {
    let target_elements: BTreeMap<_, _> = target
        .all_items()
        .map(|item| (item.element_id.as_str(), item))
        .collect();
    for item in source.all_items() {
        if let Some(other) = target_elements.get(item.element_id.as_str())
            && (item.path != other.path || item.value != other.value)
        {
            return Err(StorageError::Conflict("append_element_identity"));
        }
    }
    let mut source_operations = operation_identities(source);
    let target_operations = operation_identities(target);
    for (operation_id, source_identity) in &mut source_operations {
        source_identity.sort();
        if let Some(target_identity) = target_operations.get(operation_id) {
            let mut target_identity = target_identity.clone();
            target_identity.sort();
            if *source_identity != target_identity {
                return Err(StorageError::Conflict("append_operation_identity"));
            }
        }
    }
    Ok(())
}

fn operation_identities(history: &AppendHistory) -> BTreeMap<&str, Vec<(String, String, String)>> {
    let mut operations = BTreeMap::new();
    for item in history.all_items() {
        operations
            .entry(item.operation_id.as_str())
            .or_insert_with(Vec::new)
            .push((
                item.path.clone(),
                item.element_id.clone(),
                serde_json::to_string(&item.value).expect("JSON values serialize"),
            ));
    }
    operations
}
