use std::collections::{HashSet, VecDeque};

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{canonical, runtime::ContextBranchView};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

const MAX_ANCESTRY: usize = 10_000;

pub(super) async fn is_reachable<C: ConnectionTrait>(
    connection: &C,
    head: &str,
    target: &str,
) -> StorageResult<bool> {
    let mut queue = VecDeque::from([head.to_owned()]);
    let mut seen = HashSet::new();
    while let Some(commit) = queue.pop_front() {
        if commit == target {
            return Ok(true);
        }
        if !seen.insert(commit.clone()) {
            continue;
        }
        if seen.len() > MAX_ANCESTRY {
            return Err(StorageError::Integrity(
                "context ancestry exceeds traversal limit".into(),
            ));
        }
        for row in connection.query_all_raw(sql(
            "SELECT parent_commit_id FROM commit_parents WHERE commit_id = ? ORDER BY parent_order",
            vec![commit.into()],
        )).await? {
            queue.push_back(row.try_get("", "parent_commit_id")?);
        }
    }
    Ok(false)
}

pub(super) async fn replay<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<ContextBranchView>> {
    let row = connection.query_one_raw(sql(
        "SELECT request_digest, result_object_id FROM application_command_receipts WHERE scope = ? AND idempotency_key = ? AND status = 'completed'",
        vec![scope.into(), key.into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    load_object_json(connection, &row.try_get::<String>("", "result_object_id")?)
        .await
        .map(Some)
}

pub(super) async fn verify_replay<C: ConnectionTrait>(
    connection: &C,
    view: &ContextBranchView,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT fork_commit_id FROM context_branches WHERE context_id = ? AND id = ?",
            vec![
                view.context_id.clone().into(),
                view.branch_id.clone().into(),
            ],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("fork receipt branch is missing".into()))?;
    if row.try_get::<String>("", "fork_commit_id")? != view.fork_commit_id {
        return Err(StorageError::Integrity(
            "fork receipt branch changed".into(),
        ));
    }
    Ok(())
}

pub(super) async fn finish_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
    result: &ContextBranchView,
    now: i64,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(result)?, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES (?, ?, ?, 'context.fork', 'context_branch', ?, 'completed', ?, ?, ?)",
        vec![scope.into(), key.into(), digest.into(), result.branch_id.clone().into(), object_id.clone().into(), now.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'application_receipt', ?, 'result', ?)",
        vec![object_id.into(), format!("{scope}:{key}").into(), now.into()],
    )).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn append_event<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    branch_id: &str,
    source_branch_id: &str,
    commit_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO domain_event_counters (aggregate_kind, aggregate_id, lineage_key, next_seq) VALUES ('working_context', ?, ?, 2)", vec![context_id.into(), branch_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO domain_events (id, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json, created_at) VALUES (?, 'working_context', ?, ?, 1, 'context.branch.created', 1, ?, ?)",
        vec![new_id("domain_event").into(), context_id.into(), branch_id.into(), canonical::to_string(&json!({"schemaVersion":1,"branchId":branch_id,"sourceBranchId":source_branch_id,"forkCommitId":commit_id}))?.into(), now.into()],
    )).await?;
    Ok(())
}
