use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeAppendItem {
    pub path: String,
    pub element_id: String,
    pub value: Value,
    pub branch_id: String,
    pub sequence_no: i64,
    pub operation_id: String,
    pub operation_index: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppendArrayMerge {
    pub value: Value,
    pub integrated_source: Vec<MergeAppendItem>,
}

pub fn merge_append_only_arrays(
    path: &str,
    base: &Value,
    source: &Value,
    target: &Value,
    source_items: &[MergeAppendItem],
    target_items: &[MergeAppendItem],
) -> Option<AppendArrayMerge> {
    let (Value::Array(base), Value::Array(source), Value::Array(target)) = (base, source, target)
    else {
        return None;
    };
    if !source.starts_with(base) || !target.starts_with(base) {
        return None;
    }
    let source_items = unique_for_path(path, source_items)?;
    let target_items = unique_for_path(path, target_items)?;
    if !tail_matches(&source[base.len()..], source_items.values())?
        || !tail_matches(&target[base.len()..], target_items.values())?
    {
        return None;
    }
    let mut combined = target_items.clone();
    for (element_id, source_item) in &source_items {
        if let Some(target_item) = combined.get(element_id)
            && target_item.value != source_item.value
        {
            return None;
        }
        combined
            .entry(element_id.clone())
            .and_modify(|current| {
                if stable_key(source_item) < stable_key(current) {
                    *current = source_item.clone();
                }
            })
            .or_insert_with(|| source_item.clone());
    }
    let mut ordered: Vec<_> = combined.into_values().collect();
    ordered.sort_by(|left, right| stable_key(left).cmp(&stable_key(right)));
    let mut merged = base.clone();
    merged.extend(ordered.into_iter().map(|item| item.value));
    let integrated_source = source_items
        .into_iter()
        .filter(|(element_id, source_item)| {
            target_items
                .get(element_id)
                .is_none_or(|target_item| stable_key(source_item) < stable_key(target_item))
        })
        .map(|(_, item)| item)
        .collect();
    Some(AppendArrayMerge {
        value: Value::Array(merged),
        integrated_source,
    })
}

fn unique_for_path(
    path: &str,
    items: &[MergeAppendItem],
) -> Option<BTreeMap<String, MergeAppendItem>> {
    let mut unique: BTreeMap<String, MergeAppendItem> = BTreeMap::new();
    for item in items.iter().filter(|item| item.path == path) {
        if let Some(existing) = unique.get(&item.element_id)
            && existing.value != item.value
        {
            return None;
        }
        unique
            .entry(item.element_id.clone())
            .or_insert_with(|| item.clone());
    }
    Some(unique)
}

fn tail_matches<'a>(
    tail: &[Value],
    items: impl Iterator<Item = &'a MergeAppendItem>,
) -> Option<bool> {
    let mut expected = BTreeMap::<String, usize>::new();
    for item in items {
        *expected
            .entry(canonical::hash(&item.value).ok()?)
            .or_default() += 1;
    }
    let mut actual = BTreeMap::<String, usize>::new();
    for value in tail {
        *actual.entry(canonical::hash(value).ok()?).or_default() += 1;
    }
    Some(actual == expected)
}

fn stable_key(item: &MergeAppendItem) -> (&str, i64, &str, u32, &str) {
    (
        &item.branch_id,
        item.sequence_no,
        &item.operation_id,
        item.operation_index,
        &item.element_id,
    )
}
