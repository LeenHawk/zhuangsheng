use std::collections::HashSet;

use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::Value;
use zhuangsheng_core::{
    application::context::WorkingContextView,
    canonical,
    state::{JsonPatchOp, StatePatch, apply_patch},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, now_ms, sql},
};

use super::{commit::effective_patch, merge_append_history::load_resolution_append_ids};

const MAX_HISTORY_COMMITS: usize = 10_000;
const MAX_PROJECTION_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct ReconstructedContext {
    pub context_id: String,
    pub branch_id: String,
    pub value: Value,
    pub append_ids: HashSet<String>,
}

pub(crate) async fn reconstruct<C: ConnectionTrait>(
    connection: &C,
    commit_id: &str,
) -> StorageResult<ReconstructedContext> {
    let mut current = commit_id.to_string();
    let mut patches = Vec::new();
    let mut visited = 0;
    let mut target_identity = None;
    let (mut value, mut append_ids) = loop {
        visited += 1;
        if visited > MAX_HISTORY_COMMITS {
            return Err(StorageError::Integrity(
                "context replay exceeds history limit".into(),
            ));
        }
        let row = connection.query_one_raw(sql(
            "SELECT v.aggregate_id, v.lineage_key, v.patch_object_id, v.initial_snapshot_object_id, v.merge_resolution_object_id, s.snapshot_object_id AS version_snapshot_id, s.schema_version AS snapshot_schema_version, s.checksum, p.parent_commit_id FROM version_commits v LEFT JOIN version_snapshots s ON s.commit_id = v.id LEFT JOIN commit_parents p ON p.commit_id = v.id AND p.parent_order = 0 WHERE v.id = ? AND v.aggregate_kind = 'working_context'",
            vec![current.clone().into()],
        )).await?.ok_or_else(|| StorageError::NotFound {
            kind: "context_commit",
            id: current.clone(),
        })?;
        let context_id: String = row.try_get("", "aggregate_id")?;
        let branch_id: String = row.try_get("", "lineage_key")?;
        target_identity.get_or_insert((context_id.clone(), branch_id));
        if target_identity
            .as_ref()
            .is_some_and(|(expected, _)| expected != &context_id)
        {
            return Err(StorageError::Integrity(
                "context replay crosses aggregate boundary".into(),
            ));
        }
        let version_snapshot: Option<String> = row.try_get("", "version_snapshot_id")?;
        if let Some(snapshot_id) = version_snapshot {
            let schema: i64 = row.try_get("", "snapshot_schema_version")?;
            if schema != 1 {
                return Err(StorageError::Integrity(
                    "unsupported VersionSnapshot schema".into(),
                ));
            }
            let envelope: Value = load_object_json(connection, &snapshot_id).await?;
            let checksum: String = row.try_get("", "checksum")?;
            if canonical::hash(&envelope)? != checksum {
                return Err(StorageError::Integrity(
                    "VersionSnapshot checksum mismatch".into(),
                ));
            }
            let value = envelope
                .get("value")
                .cloned()
                .ok_or_else(|| StorageError::Integrity("snapshot value missing".into()))?;
            let ids = envelope
                .get("appendElementIds")
                .and_then(Value::as_array)
                .ok_or_else(|| StorageError::Integrity("snapshot append IDs missing".into()))?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(String::from)
                        .ok_or_else(|| StorageError::Integrity("invalid snapshot append ID".into()))
                })
                .collect::<Result<HashSet<_>, _>>()?;
            break (value, ids);
        }
        let initial: Option<String> = row.try_get("", "initial_snapshot_object_id")?;
        if let Some(snapshot_id) = initial {
            break (
                load_object_json(connection, &snapshot_id).await?,
                HashSet::new(),
            );
        }
        let patch_id: Option<String> = row.try_get("", "patch_object_id")?;
        let parent: Option<String> = row.try_get("", "parent_commit_id")?;
        let (Some(patch_id), Some(parent)) = (patch_id, parent) else {
            return Err(StorageError::Integrity(
                "non-root context commit lacks patch or parent".into(),
            ));
        };
        patches.push((
            patch_id,
            row.try_get::<Option<String>>("", "merge_resolution_object_id")?,
        ));
        current = parent;
    };
    for (patch_id, resolution_id) in patches.into_iter().rev() {
        let patch: StatePatch = load_object_json(connection, &patch_id).await?;
        let effective = effective_patch(&patch, &append_ids);
        if !effective.ops.is_empty() {
            value = apply_patch(&value, &effective)?;
        }
        for operation in patch.ops {
            if let JsonPatchOp::Append { element_id, .. } = operation {
                append_ids.insert(element_id);
            }
        }
        if let Some(resolution_id) = resolution_id {
            append_ids.extend(load_resolution_append_ids(connection, &resolution_id).await?);
        }
    }
    if canonical::to_vec(&value)?.len() > MAX_PROJECTION_BYTES {
        return Err(StorageError::Integrity(
            "reconstructed context exceeds projection limit".into(),
        ));
    }
    let (context_id, branch_id) = target_identity.expect("at least one commit row");
    Ok(ReconstructedContext {
        context_id,
        branch_id,
        value,
        append_ids,
    })
}

impl SqliteStore {
    pub async fn get_context_at_commit(
        &self,
        commit_id: &str,
    ) -> StorageResult<WorkingContextView> {
        let reconstructed = reconstruct(&self.db, commit_id).await?;
        Ok(WorkingContextView {
            context_id: reconstructed.context_id,
            branch_id: reconstructed.branch_id,
            head_commit_id: commit_id.into(),
            value: reconstructed.value,
        })
    }

    pub async fn rebuild_working_context_projection(
        &self,
        context_id: &str,
        branch_id: &str,
        expected_head: &str,
    ) -> StorageResult<WorkingContextView> {
        let transaction = self.db.begin().await?;
        let reconstructed = reconstruct(&transaction, expected_head).await?;
        if reconstructed.context_id != context_id {
            return Err(StorageError::Conflict("context_projection_identity"));
        }
        let projection = canonical::to_string(&reconstructed.value)?;
        let updated = transaction.execute_raw(sql(
            "UPDATE materialized_projections SET projection_json = ?, projection_object_id = NULL, schema_version = 1, updated_at = ? WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND head_commit_id = ? AND EXISTS (SELECT 1 FROM context_branches WHERE context_id = ? AND id = ? AND head_commit_id = ?)",
            vec![projection.into(), now_ms().into(), context_id.into(), branch_id.into(), expected_head.into(), context_id.into(), branch_id.into(), expected_head.into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("context_projection_head"));
        }
        transaction.commit().await?;
        Ok(WorkingContextView {
            context_id: context_id.into(),
            branch_id: branch_id.into(),
            head_commit_id: expected_head.into(),
            value: reconstructed.value,
        })
    }
}
