use std::collections::{HashMap, HashSet};

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    context_merge::{
        ExplicitMergeResolution, ExplicitMergeSelection, MergeConflictView, MergePathConflict,
    },
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
};

pub(super) struct MergeIdentity<'a> {
    pub context_id: &'a str,
    pub source_branch_id: &'a str,
    pub target_branch_id: &'a str,
    pub base_commit_id: &'a str,
    pub source_head: &'a str,
    pub target_head: &'a str,
}

pub(super) struct ResolvedMerge {
    pub value: Value,
    pub resolutions: Vec<(String, Value)>,
    pub append_entries: Vec<zhuangsheng_core::context_merge::MergeAppendItem>,
    pub blocked_paths: Vec<String>,
}

pub(super) async fn persist_conflicts<C: ConnectionTrait>(
    connection: &C,
    identity: &MergeIdentity<'_>,
    conflicts: &[MergePathConflict],
    now: i64,
) -> StorageResult<Vec<MergeConflictView>> {
    let mut views = Vec::with_capacity(conflicts.len());
    for conflict in conflicts {
        let id = conflict_id(identity, &conflict.path)?;
        let existing = connection.query_one_raw(sql(
            "SELECT context_id, source_branch_id, target_branch_id, base_commit_id, source_head_commit_id, target_head_commit_id, path, base_value_object_id, source_value_object_id, target_value_object_id, status FROM context_merge_conflicts WHERE id = ?",
            vec![id.clone().into()],
        )).await?;
        if let Some(row) = existing {
            verify_existing(connection, &row, identity, conflict).await?;
        } else {
            let base = put_value(connection, &conflict.base, now).await?;
            let source = put_value(connection, &conflict.source, now).await?;
            let target = put_value(connection, &conflict.target, now).await?;
            connection.execute_raw(sql(
                "INSERT INTO context_merge_conflicts (id, context_id, source_branch_id, target_branch_id, base_commit_id, source_head_commit_id, target_head_commit_id, path, base_value_object_id, source_value_object_id, target_value_object_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'open', ?)",
                vec![id.clone().into(), identity.context_id.into(), identity.source_branch_id.into(), identity.target_branch_id.into(), identity.base_commit_id.into(), identity.source_head.into(), identity.target_head.into(), conflict.path.clone().into(), base.clone().into(), source.clone().into(), target.clone().into(), now.into()],
            )).await?;
            for (object_id, role) in [
                (base, "merge_base"),
                (source, "merge_source"),
                (target, "merge_target"),
            ] {
                connection.execute_raw(sql(
                    "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'merge_conflict', ?, ?, ?)",
                    vec![object_id.into(), id.clone().into(), role.into(), now.into()],
                )).await?;
            }
        }
        views.push(MergeConflictView {
            conflict_id: id,
            path: conflict.path.clone(),
            base: conflict.base.clone(),
            source: conflict.source.clone(),
            target: conflict.target.clone(),
        });
    }
    Ok(views)
}

async fn verify_existing<C: ConnectionTrait>(
    connection: &C,
    row: &sea_orm::QueryResult,
    identity: &MergeIdentity<'_>,
    conflict: &MergePathConflict,
) -> StorageResult<()> {
    let identity_matches = row.try_get::<String>("", "context_id")? == identity.context_id
        && row.try_get::<String>("", "source_branch_id")? == identity.source_branch_id
        && row.try_get::<String>("", "target_branch_id")? == identity.target_branch_id
        && row.try_get::<String>("", "base_commit_id")? == identity.base_commit_id
        && row.try_get::<String>("", "source_head_commit_id")? == identity.source_head
        && row.try_get::<String>("", "target_head_commit_id")? == identity.target_head
        && row.try_get::<String>("", "path")? == conflict.path;
    if !identity_matches {
        return Err(StorageError::Integrity(
            "merge conflict identity is corrupt".into(),
        ));
    }
    if row.try_get::<String>("", "status")? != "open" {
        return Err(StorageError::Conflict("merge_conflict_not_open"));
    }
    for (column, expected) in [
        ("base_value_object_id", &conflict.base),
        ("source_value_object_id", &conflict.source),
        ("target_value_object_id", &conflict.target),
    ] {
        let stored: Value =
            load_object_json(connection, &row.try_get::<String>("", column)?).await?;
        if stored != json!({"present":expected.is_some(),"value":expected}) {
            return Err(StorageError::Integrity(
                "merge conflict values are corrupt".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn resolve(
    mut merged: Value,
    conflicts: &[MergeConflictView],
    selections: &[ExplicitMergeSelection],
    append_entries: Vec<zhuangsheng_core::context_merge::MergeAppendItem>,
    mut blocked_paths: Vec<String>,
) -> StorageResult<Result<ResolvedMerge, Vec<MergeConflictView>>> {
    let by_id: HashMap<_, _> = conflicts
        .iter()
        .map(|item| (item.conflict_id.as_str(), item))
        .collect();
    let mut selected = HashSet::new();
    let mut resolutions = Vec::new();
    for selection in selections {
        let conflict = by_id.get(selection.conflict_id.as_str()).ok_or_else(|| {
            StorageError::InvalidArgument("merge selection references an unknown conflict".into())
        })?;
        if conflict.path != selection.path || !selected.insert(selection.conflict_id.as_str()) {
            return Err(StorageError::InvalidArgument(
                "merge selection identity is invalid".into(),
            ));
        }
        let value = match &selection.resolution {
            ExplicitMergeResolution::FinalValue { value } => value.clone(),
            ExplicitMergeResolution::ArtifactRef { artifact_ref } => {
                serde_json::to_value(artifact_ref)
                    .map_err(|error| StorageError::Integrity(error.to_string()))?
            }
        };
        super::merge_pointer::set_pointer(&mut merged, &selection.path, value.clone())?;
        blocked_paths.push(selection.path.clone());
        resolutions.push((selection.conflict_id.clone(), value));
    }
    let missing: Vec<_> = conflicts
        .iter()
        .filter(|item| !selected.contains(item.conflict_id.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        blocked_paths.sort();
        blocked_paths.dedup();
        Ok(Ok(ResolvedMerge {
            value: merged,
            resolutions,
            append_entries,
            blocked_paths,
        }))
    } else {
        Ok(Err(conflicts.to_vec()))
    }
}

fn conflict_id(identity: &MergeIdentity<'_>, path: &str) -> StorageResult<String> {
    let hash = canonical::hash(&json!({
        "schemaVersion":1,"contextId":identity.context_id,
        "sourceBranchId":identity.source_branch_id,"targetBranchId":identity.target_branch_id,
        "baseCommitId":identity.base_commit_id,"sourceHead":identity.source_head,
        "targetHead":identity.target_head,"path":path,
    }))?;
    Ok(format!("mergeconflict_{}", &hash[7..]))
}

async fn put_value<C: ConnectionTrait>(
    connection: &C,
    value: &Option<Value>,
    now: i64,
) -> StorageResult<String> {
    put_inline_object(
        connection,
        &canonical::to_vec(&json!({"present":value.is_some(),"value":value}))?,
        now,
    )
    .await
}
