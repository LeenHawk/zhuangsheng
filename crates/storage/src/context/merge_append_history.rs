use std::collections::{BTreeMap, HashSet};

use sea_orm::ConnectionTrait;
use serde::Deserialize;
use zhuangsheng_core::{
    context_merge::MergeAppendItem,
    state::{JsonPatchOp, StatePatch},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

const MAX_COMMITS: usize = 10_000;

#[derive(Default)]
pub(super) struct AppendHistory {
    by_path: BTreeMap<String, Vec<MergeAppendItem>>,
    blocked_paths: HashSet<String>,
}

impl AppendHistory {
    pub fn items(&self, path: &str) -> &[MergeAppendItem] {
        self.by_path.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn append_paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }

    pub fn all_items(&self) -> impl Iterator<Item = &MergeAppendItem> {
        self.by_path.values().flatten()
    }

    pub fn is_append_only(&self, path: &str) -> bool {
        !self
            .blocked_paths
            .iter()
            .any(|blocked| paths_overlap(blocked, path))
    }

    pub fn blocked_paths(&self) -> impl Iterator<Item = &str> {
        self.blocked_paths.iter().map(String::as_str)
    }
}

struct Step {
    branch_id: String,
    sequence_no: i64,
    operation_id: String,
    patch_object_id: Option<String>,
    resolution_object_id: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeProvenance {
    #[serde(default)]
    append_entries: Vec<MergeAppendItem>,
    #[serde(default)]
    blocked_paths: Vec<String>,
}

pub(super) async fn load_resolution_append_ids<C: ConnectionTrait>(
    connection: &C,
    resolution_object_id: &str,
) -> StorageResult<HashSet<String>> {
    let provenance: MergeProvenance = load_object_json(connection, resolution_object_id).await?;
    Ok(provenance
        .append_entries
        .into_iter()
        .map(|item| item.element_id)
        .collect())
}

pub(super) async fn load_append_history<C: ConnectionTrait>(
    connection: &C,
    head: &str,
    base: &str,
    base_ids: &HashSet<String>,
) -> StorageResult<AppendHistory> {
    let mut current = head.to_owned();
    let mut reverse = Vec::new();
    while current != base {
        if reverse.len() >= MAX_COMMITS {
            return Err(StorageError::Integrity(
                "merge append history exceeds traversal limit".into(),
            ));
        }
        let row = connection.query_one_raw(sql(
            "SELECT v.lineage_key, v.sequence_no, v.operation_id, v.patch_object_id, v.merge_resolution_object_id, p.parent_commit_id FROM version_commits v LEFT JOIN commit_parents p ON p.commit_id = v.id AND p.parent_order = 0 WHERE v.id = ? AND v.aggregate_kind = 'working_context'",
            vec![current.clone().into()],
        )).await?.ok_or_else(|| StorageError::Integrity("merge append ancestry is broken".into()))?;
        reverse.push(Step {
            branch_id: row.try_get("", "lineage_key")?,
            sequence_no: row.try_get("", "sequence_no")?,
            operation_id: row.try_get("", "operation_id")?,
            patch_object_id: row.try_get("", "patch_object_id")?,
            resolution_object_id: row.try_get("", "merge_resolution_object_id")?,
        });
        let Some(parent) = row.try_get::<Option<String>>("", "parent_commit_id")? else {
            return Ok(AppendHistory {
                blocked_paths: HashSet::from([String::new()]),
                ..AppendHistory::default()
            });
        };
        current = parent;
    }
    let mut history = AppendHistory::default();
    let mut seen = base_ids.clone();
    for step in reverse.into_iter().rev() {
        apply_step(connection, &mut history, &mut seen, step).await?;
    }
    Ok(history)
}

async fn apply_step<C: ConnectionTrait>(
    connection: &C,
    history: &mut AppendHistory,
    seen: &mut HashSet<String>,
    step: Step,
) -> StorageResult<()> {
    if let Some(patch_id) = step.patch_object_id {
        let patch: StatePatch = load_object_json(connection, &patch_id).await?;
        for (index, operation) in patch.ops.into_iter().enumerate() {
            match operation {
                JsonPatchOp::Append {
                    path,
                    element_id,
                    value,
                } if seen.insert(element_id.clone()) => {
                    history
                        .by_path
                        .entry(path.clone())
                        .or_default()
                        .push(MergeAppendItem {
                            path,
                            element_id,
                            value,
                            branch_id: step.branch_id.clone(),
                            sequence_no: step.sequence_no,
                            operation_id: step.operation_id.clone(),
                            operation_index: index as u32,
                        });
                }
                JsonPatchOp::Append { .. } | JsonPatchOp::Test { .. } => {}
                operation if step.resolution_object_id.is_none() => {
                    history.blocked_paths.insert(operation.path().into());
                }
                _ => {}
            }
        }
    }
    if let Some(resolution_id) = step.resolution_object_id {
        let provenance: MergeProvenance = load_object_json(connection, &resolution_id).await?;
        history.blocked_paths.extend(provenance.blocked_paths);
        for item in provenance.append_entries {
            if seen.insert(item.element_id.clone()) {
                history
                    .by_path
                    .entry(item.path.clone())
                    .or_default()
                    .push(item);
            } else {
                replace_origin(history, item)?;
            }
        }
    }
    Ok(())
}

fn replace_origin(history: &mut AppendHistory, item: MergeAppendItem) -> StorageResult<()> {
    let existing = history
        .by_path
        .values_mut()
        .flatten()
        .find(|existing| existing.element_id == item.element_id);
    let Some(existing) = existing else {
        return Ok(());
    };
    if existing.path != item.path || existing.value != item.value {
        return Err(StorageError::Integrity(
            "merge append provenance identity is corrupt".into(),
        ));
    }
    if stable_key(&item) < stable_key(existing) {
        *existing = item;
    }
    Ok(())
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

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left.is_empty()
        || right.is_empty()
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
