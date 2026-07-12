use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode, MemoryReadConsistency, RouterReadSource, RunLimits},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{
    events::{Event, add_object_ref, append_event, enqueue_wakeup, finish_wakeup},
    read_set::resolve_router_reads,
};

pub(super) enum ReconcileOutcome {
    Continue,
    Requeued,
    Exhausted,
}

pub(super) struct ReconcileAttempt<'a> {
    pub run_id: &'a str,
    pub node_instance_id: &'a str,
    pub attempt_id: &'a str,
    pub wakeup_id: &'a str,
    pub worker_id: &'a str,
    pub lease_fence: u64,
    pub run_control_epoch: u64,
    pub result_idempotency_key: &'a str,
}

pub(super) async fn reconcile_if_stale<C: ConnectionTrait>(
    connection: &C,
    attempt: ReconcileAttempt<'_>,
    node: &GraphNode,
    run_limits: &RunLimits,
    now: i64,
) -> StorageResult<ReconcileOutcome> {
    if !matches!(&node.kind, DraftNodeKind::Router { .. })
        || !has_conflict(connection, attempt.attempt_id, node).await?
    {
        return Ok(ReconcileOutcome::Continue);
    }
    let row = connection.query_one_raw(sql(
        "SELECT attempt_no, executor_object_id, (SELECT COUNT(*) FROM node_attempts WHERE node_instance_id = ? AND invocation_kind = 'reconcile') AS reconcile_count FROM node_attempts WHERE id = ?",
        vec![attempt.node_instance_id.into(), attempt.attempt_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("Router reconcile attempt missing".into()))?;
    let attempt_no: i64 = row.try_get("", "attempt_no")?;
    let executor_id: String = row.try_get("", "executor_object_id")?;
    let reconcile_count: i64 = row.try_get("", "reconcile_count")?;
    let maximum = match &node.kind {
        DraftNodeKind::Router {
            limits: Some(limits),
            ..
        } => limits.max_read_reconciles.unwrap_or(2),
        _ => 0,
    };
    if reconcile_count as u64 >= maximum
        || attempt_no as u64 >= run_limits.max_attempts_per_activation
    {
        return Ok(ReconcileOutcome::Exhausted);
    }
    let error = canonical::to_vec(&json!({
        "schemaVersion":1,
        "code":"router_read_conflict",
        "safeMessage":"Router validate-on-commit read changed",
        "retryClass":"reconcile"
    }))?;
    let error_id = put_inline_object(connection, &error, now).await?;
    let failed = connection.execute_raw(sql(
        "UPDATE node_attempts SET status = 'failed', result_idempotency_key = ?, error_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status = 'running' AND worker_id = ? AND lease_fence = ? AND run_control_epoch = ?",
        vec![attempt.result_idempotency_key.into(), error_id.clone().into(), now.into(), attempt.attempt_id.into(), attempt.worker_id.into(), (attempt.lease_fence as i64).into(), (attempt.run_control_epoch as i64).into()],
    )).await?;
    if failed.rows_affected() != 1 {
        return Err(StorageError::Conflict("attempt_fence"));
    }
    connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
        vec![attempt.attempt_id.into()],
    )).await?;
    let ready = connection.execute_raw(sql(
        "UPDATE node_instances SET status = 'ready', updated_at = ? WHERE id = ? AND status = 'running'",
        vec![now.into(), attempt.node_instance_id.into()],
    )).await?;
    if ready.rows_affected() != 1 {
        return Err(StorageError::Conflict("node_instance_status"));
    }
    let next_attempt = new_id("attempt");
    connection.execute_raw(sql(
        "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) VALUES (?, ?, ?, 0, 'reconcile', 'queued', ?, 0, ?, ?)",
        vec![next_attempt.clone().into(), attempt.node_instance_id.into(), (attempt_no + 1).into(), (attempt.run_control_epoch as i64).into(), format!("attempt:{}:{}", attempt.node_instance_id, attempt_no + 1).into(), executor_id.into()],
    )).await?;
    resolve_router_reads(connection, attempt.run_id, &next_attempt, node, now).await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET total_attempts = total_attempts + 1 WHERE run_id = ?",
        vec![attempt.run_id.into()],
    )).await?;
    add_object_ref(
        connection,
        &error_id,
        "node_attempt",
        attempt.attempt_id,
        "error",
        now,
    )
    .await?;
    let seq = append_event(
        connection,
        Event {
            run_id: attempt.run_id,
            event_type: "router.read_conflict",
            importance: "critical",
            node_instance_id: Some(attempt.node_instance_id),
            attempt_id: Some(attempt.attempt_id),
            payload: json!({"schemaVersion":1,"replacementAttemptId":next_attempt}),
            now,
        },
    )
    .await?;
    finish_wakeup(connection, attempt.wakeup_id).await?;
    enqueue_wakeup(
        connection,
        attempt.run_id,
        Some(&node.id),
        "attempt_ready",
        seq,
        &format!("attempt-ready:{next_attempt}"),
        now,
    )
    .await?;
    Ok(ReconcileOutcome::Requeued)
}

async fn has_conflict<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    node: &GraphNode,
) -> StorageResult<bool> {
    let head_changed = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM node_read_set rs LEFT JOIN context_branches b ON rs.aggregate_kind = 'working_context' AND b.context_id = rs.aggregate_id AND b.id = rs.lineage_key LEFT JOIN memory_records m ON rs.aggregate_kind = 'long_term_memory' AND m.id = rs.aggregate_id LEFT JOIN materialized_projections p ON rs.aggregate_kind = 'artifact_metadata' AND p.aggregate_kind = 'artifact_metadata' AND p.aggregate_id = rs.aggregate_id AND p.lineage_key = rs.lineage_key WHERE rs.node_attempt_id = ? AND rs.consistency = 'validate_on_commit' AND ((rs.aggregate_kind = 'working_context' AND (b.head_commit_id IS NULL OR b.head_commit_id != rs.commit_id)) OR (rs.aggregate_kind = 'long_term_memory' AND (m.head_commit_id IS NULL OR m.head_commit_id != rs.commit_id)) OR (rs.aggregate_kind = 'artifact_metadata' AND (p.head_commit_id IS NULL OR p.head_commit_id != rs.commit_id))) LIMIT 1",
        vec![attempt_id.into()],
    )).await?.is_some();
    if head_changed {
        return Ok(true);
    }
    let DraftNodeKind::Router {
        memory: Some(memory),
        ..
    } = &node.kind
    else {
        return Ok(false);
    };
    for read in &memory.reads {
        let RouterReadSource::LongTermMemory { scope, .. } = &read.source else {
            continue;
        };
        if read.consistency != MemoryReadConsistency::ValidateOnCommit {
            continue;
        }
        let row = connection.query_one_raw(sql(
            "SELECT scope_snapshot_token FROM node_bound_read_results WHERE node_attempt_id = ? AND binding_id = ?",
            vec![attempt_id.into(), read.id.clone().into()],
        )).await?.ok_or_else(|| StorageError::Integrity("Router scope snapshot missing".into()))?;
        let expected: String = row.try_get("", "scope_snapshot_token")?;
        let scope_row = connection
            .query_one_raw(sql(
                "SELECT revision_no FROM memory_scopes WHERE id = ?",
                vec![scope.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::Integrity("memory scope missing".into()))?;
        let revision: i64 = scope_row.try_get("", "revision_no")?;
        if expected != format!("memory-scope:{scope}:revision:{revision}") {
            return Ok(true);
        }
    }
    Ok(false)
}
