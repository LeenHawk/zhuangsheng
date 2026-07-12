use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    context_merge::{MergeContextCommand, MergeSourceDisposition},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{merge_conflict::ResolvedMerge, merge_event::append_event};

const MAX_PROJECTION_BYTES: usize = 16 * 1024 * 1024;

pub(super) async fn commit_merge<C: ConnectionTrait>(
    connection: &C,
    command: &MergeContextCommand,
    base_commit_id: &str,
    resolved: &ResolvedMerge,
    now: i64,
) -> StorageResult<String> {
    let projection = canonical::to_string(&resolved.value)?;
    if projection.len() > MAX_PROJECTION_BYTES {
        return Err(StorageError::InvalidArgument(
            "merged context exceeds projection limit".into(),
        ));
    }
    let operation_hash = canonical::hash(&json!({
        "contextId":command.context_id,"sourceBranchId":command.source_branch_id,
        "targetBranchId":command.target_branch_id,"baseCommitId":base_commit_id,
        "sourceHead":command.expected_source_head,"targetHead":command.expected_target_head,
    }))?;
    let patch = StatePatch {
        aggregate_kind: AggregateKind::WorkingContext,
        aggregate_id: command.context_id.clone(),
        lineage_key: command.target_branch_id.clone(),
        base_commit_id: command.expected_target_head.clone(),
        operation_id: format!("merge-context:{}", &operation_hash[7..]),
        ops: vec![JsonPatchOp::Replace {
            path: String::new(),
            value: resolved.value.clone(),
        }],
        schema_version: 1,
        policy_version: 1,
        author: ActorRef {
            kind: ActorKind::Application,
            id: None,
        },
    };
    let patch_id = put_inline_object(connection, &canonical::to_vec(&patch)?, now).await?;
    let resolution_id = put_inline_object(
        connection,
        &canonical::to_vec(&json!({
            "schemaVersion":1,"baseCommitId":base_commit_id,
            "sourceHeadCommitId":command.expected_source_head,
            "targetHeadCommitId":command.expected_target_head,
            "sourceDisposition":command.source_disposition,
            "resolutions":resolved.resolutions,
            "appendEntries":resolved.append_entries,
            "blockedPaths":resolved.blocked_paths,
        }))?,
        now,
    )
    .await?;
    let sequence = next_sequence(connection, &command.expected_target_head).await?;
    let commit_id = new_id("commit");
    connection.execute_raw(sql(
        "INSERT INTO version_commits (id, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, patch_object_id, merge_resolution_object_id, schema_version, policy_version, author_kind, created_at) VALUES (?, 'working_context', ?, ?, ?, ?, ?, ?, 1, 1, 'application', ?)",
        vec![commit_id.clone().into(), command.context_id.clone().into(), command.target_branch_id.clone().into(), sequence.into(), patch.operation_id.into(), patch_id.clone().into(), resolution_id.clone().into(), now.into()],
    )).await?;
    for (order, parent) in [
        command.expected_target_head.as_str(),
        command.expected_source_head.as_str(),
    ]
    .into_iter()
    .enumerate()
    {
        connection.execute_raw(sql(
            "INSERT INTO commit_parents (commit_id, parent_commit_id, parent_order) VALUES (?, ?, ?)",
            vec![commit_id.clone().into(), parent.into(), (order as i64).into()],
        )).await?;
    }
    advance_branches(connection, command, &commit_id, &projection, now).await?;
    for (object_id, role) in [
        (patch_id, "patch"),
        (resolution_id.clone(), "merge_resolution"),
    ] {
        connection.execute_raw(sql(
            "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'version_commit', ?, ?, ?)",
            vec![object_id.into(), commit_id.clone().into(), role.into(), now.into()],
        )).await?;
    }
    resolve_conflicts(connection, &resolved.resolutions, &resolution_id, now).await?;
    append_event(
        connection,
        command,
        base_commit_id,
        &commit_id,
        sequence,
        now,
    )
    .await?;
    Ok(commit_id)
}

async fn advance_branches<C: ConnectionTrait>(
    connection: &C,
    command: &MergeContextCommand,
    commit_id: &str,
    projection: &str,
    now: i64,
) -> StorageResult<()> {
    let target = connection.execute_raw(sql(
        "UPDATE context_branches SET head_commit_id = ?, updated_at = ? WHERE context_id = ? AND id = ? AND head_commit_id = ? AND status = 'active'",
        vec![commit_id.into(), now.into(), command.context_id.clone().into(), command.target_branch_id.clone().into(), command.expected_target_head.clone().into()],
    )).await?;
    if target.rows_affected() != 1 {
        return Err(StorageError::Conflict("context_head"));
    }
    let projection_update = connection.execute_raw(sql(
        "UPDATE materialized_projections SET head_commit_id = ?, projection_json = ?, projection_object_id = NULL, schema_version = 1, updated_at = ? WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND head_commit_id = ?",
        vec![commit_id.into(), projection.into(), now.into(), command.context_id.clone().into(), command.target_branch_id.clone().into(), command.expected_target_head.clone().into()],
    )).await?;
    if projection_update.rows_affected() != 1 {
        return Err(StorageError::Conflict("context_projection_head"));
    }
    let source_status = match command.source_disposition {
        MergeSourceDisposition::MarkMerged => "merged",
        MergeSourceDisposition::KeepActive => "active",
    };
    let source = connection.execute_raw(sql(
        "UPDATE context_branches SET status = ?, updated_at = ? WHERE context_id = ? AND id = ? AND head_commit_id = ? AND status = 'active'",
        vec![source_status.into(), now.into(), command.context_id.clone().into(), command.source_branch_id.clone().into(), command.expected_source_head.clone().into()],
    )).await?;
    if source.rows_affected() != 1 {
        return Err(StorageError::Conflict("context_head"));
    }
    Ok(())
}

async fn next_sequence<C: ConnectionTrait>(connection: &C, head: &str) -> StorageResult<i64> {
    let row = connection
        .query_one_raw(sql(
            "SELECT sequence_no FROM version_commits WHERE id = ?",
            vec![head.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("merge target head is missing".into()))?;
    row.try_get::<i64>("", "sequence_no")?
        .checked_add(1)
        .ok_or_else(|| StorageError::Integrity("context sequence overflow".into()))
}

async fn resolve_conflicts<C: ConnectionTrait>(
    connection: &C,
    resolutions: &[(String, Value)],
    resolution_id: &str,
    now: i64,
) -> StorageResult<()> {
    for (conflict_id, _) in resolutions {
        let updated = connection.execute_raw(sql(
            "UPDATE context_merge_conflicts SET status = 'resolved', resolution_object_id = ?, resolved_at = ? WHERE id = ? AND status = 'open'",
            vec![resolution_id.into(), now.into(), conflict_id.clone().into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("merge_conflict_not_open"));
        }
        connection.execute_raw(sql(
            "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'merge_conflict', ?, 'resolution', ?)",
            vec![resolution_id.into(), conflict_id.clone().into(), now.into()],
        )).await?;
    }
    Ok(())
}
