use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::artifact::ArtifactRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeSourceDisposition {
    MarkMerged,
    KeepActive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExplicitMergeResolution {
    FinalValue { value: Value },
    ArtifactRef { artifact_ref: ArtifactRef },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitMergeSelection {
    pub conflict_id: String,
    pub path: String,
    pub resolution: ExplicitMergeResolution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeContextCommand {
    pub context_id: String,
    pub source_branch_id: String,
    pub target_branch_id: String,
    pub expected_source_head: String,
    pub expected_target_head: String,
    pub source_disposition: MergeSourceDisposition,
    #[serde(default)]
    pub selections: Vec<ExplicitMergeSelection>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeConflictView {
    pub conflict_id: String,
    pub path: String,
    pub base: Option<Value>,
    pub source: Option<Value>,
    pub target: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeContextStatus {
    Conflicted,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeContextView {
    pub context_id: String,
    pub source_branch_id: String,
    pub target_branch_id: String,
    pub base_commit_id: String,
    pub source_head_commit_id: String,
    pub target_head_commit_id: String,
    pub status: MergeContextStatus,
    pub conflicts: Vec<MergeConflictView>,
    pub merge_commit_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergePathConflict {
    pub path: String,
    pub base: Option<Value>,
    pub source: Option<Value>,
    pub target: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreeWayMergeAnalysis {
    pub merged: Value,
    pub conflicts: Vec<MergePathConflict>,
}

pub fn analyze_three_way(base: &Value, source: &Value, target: &Value) -> ThreeWayMergeAnalysis {
    let mut conflicts = Vec::new();
    let merged = merge_value("", Some(base), Some(source), Some(target), &mut conflicts)
        .unwrap_or(Value::Null);
    ThreeWayMergeAnalysis { merged, conflicts }
}

fn merge_value(
    path: &str,
    base: Option<&Value>,
    source: Option<&Value>,
    target: Option<&Value>,
    conflicts: &mut Vec<MergePathConflict>,
) -> Option<Value> {
    if source == target {
        return source.cloned();
    }
    if source == base {
        return target.cloned();
    }
    if target == base {
        return source.cloned();
    }
    if let (Some(Value::Object(base)), Some(Value::Object(source)), Some(Value::Object(target))) =
        (base, source, target)
    {
        return Some(Value::Object(merge_objects(
            path, base, source, target, conflicts,
        )));
    }
    if let (Some(Value::Array(base)), Some(Value::Array(source)), Some(Value::Array(target))) =
        (base, source, target)
        && source.starts_with(base)
        && target.starts_with(base)
    {
        let mut merged = base.clone();
        merged.extend_from_slice(&source[base.len()..]);
        merged.extend_from_slice(&target[base.len()..]);
        return Some(Value::Array(merged));
    }
    conflicts.push(MergePathConflict {
        path: path.into(),
        base: base.cloned(),
        source: source.cloned(),
        target: target.cloned(),
    });
    target.cloned()
}

fn merge_objects(
    path: &str,
    base: &Map<String, Value>,
    source: &Map<String, Value>,
    target: &Map<String, Value>,
    conflicts: &mut Vec<MergePathConflict>,
) -> Map<String, Value> {
    let keys: BTreeSet<_> = base
        .keys()
        .chain(source.keys())
        .chain(target.keys())
        .collect();
    let mut merged = Map::new();
    for key in keys {
        let child_path = format!("{path}/{}", escape(key));
        if let Some(value) = merge_value(
            &child_path,
            base.get(key),
            source.get(key),
            target.get(key),
            conflicts,
        ) {
            merged.insert(key.clone(), value);
        }
    }
    merged
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
