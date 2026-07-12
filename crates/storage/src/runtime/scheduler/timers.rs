use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use zhuangsheng_core::{canonical, scheduler::retry_delay_ms};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{load_object_json, new_id, put_inline_object, sql},
    },
};

use super::{
    aggregator_close,
    events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run},
    read_set::copy_attempt_reads,
};

impl SqliteStore {
    pub(crate) async fn process_due_timers(&self, now: i64) -> StorageResult<u64> {
        let transaction = self.db.begin().await?;
        let aggregation = transaction.query_one_raw(sql(
            "SELECT w.id, w.deadline_at AS due_at FROM aggregation_windows w JOIN graph_runs r ON r.id = w.run_id WHERE w.status = 'open' AND w.deadline_at <= ? AND r.status IN ('running','waiting') ORDER BY w.deadline_at, w.id LIMIT 1",
            vec![now.into()],
        )).await?;
        let row = transaction.query_one_raw(sql(
            "SELECT id, run_id, node_instance_id, node_attempt_id, kind, payload_object_id, due_at FROM runtime_timers WHERE status = 'pending' AND due_at <= ? ORDER BY due_at, id LIMIT 1",
            vec![now.into()],
        )).await?;
        let aggregation_is_first = match (&aggregation, &row) {
            (Some(aggregation), Some(timer)) => {
                aggregation.try_get::<i64>("", "due_at")? < timer.try_get::<i64>("", "due_at")?
            }
            (Some(_), None) => true,
            _ => false,
        };
        if aggregation_is_first {
            let aggregation = aggregation.expect("checked above");
            aggregator_close::close_due(
                &transaction,
                &aggregation.try_get::<String>("", "id")?,
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(1);
        }
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(0);
        };
        let timer_id: String = row.try_get("", "id")?;
        let run_id: String = row.try_get("", "run_id")?;
        let kind: String = row.try_get("", "kind")?;
        match kind.as_str() {
            "run_deadline" => fire_run_deadline(&transaction, &timer_id, &run_id, now).await?,
            "attempt_deadline" => {
                let attempt_id: String = row.try_get("", "node_attempt_id")?;
                fire_attempt_deadline(&transaction, &timer_id, &attempt_id, now).await?;
            }
            "retry" => {
                let instance_id: String = row.try_get("", "node_instance_id")?;
                let payload_id: String = row.try_get("", "payload_object_id")?;
                fire_retry(
                    &transaction,
                    &timer_id,
                    &run_id,
                    &instance_id,
                    &payload_id,
                    now,
                )
                .await?;
            }
            _ => return Err(StorageError::Integrity("unknown runtime timer kind".into())),
        }
        transaction.commit().await?;
        Ok(1)
    }
}

async fn fire_run_deadline<C: ConnectionTrait>(
    connection: &C,
    timer_id: &str,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    if !mark_fired(connection, timer_id, now).await? {
        return Ok(());
    }
    fail_run(
        connection,
        run_id,
        "run_deadline_exceeded",
        "run deadline exceeded",
        now,
    )
    .await
}

async fn fire_attempt_deadline<C: ConnectionTrait>(
    connection: &C,
    timer_id: &str,
    attempt_id: &str,
    now: i64,
) -> StorageResult<()> {
    let row = connection.query_one_raw(sql(
        "SELECT a.status, a.attempt_no, a.retry_ordinal, a.invocation_kind, ni.id AS node_instance_id, ni.run_id, ni.node_id, ni.graph_revision_id, r.status AS run_status FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE a.id = ?",
        vec![attempt_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("attempt deadline owner missing".into()))?;
    let status: String = row.try_get("", "status")?;
    if !matches!(status.as_str(), "leased" | "running") {
        cancel_timer(connection, timer_id).await?;
        return Ok(());
    }
    if !mark_fired(connection, timer_id, now).await? {
        return Ok(());
    }
    let instance_id: String = row.try_get("", "node_instance_id")?;
    let run_id: String = row.try_get("", "run_id")?;
    let node_id: String = row.try_get("", "node_id")?;
    let revision_id: String = row.try_get("", "graph_revision_id")?;
    let attempt_no: i64 = row.try_get("", "attempt_no")?;
    let current_ordinal: i64 = row.try_get("", "retry_ordinal")?;
    let invocation_kind: String = row.try_get("", "invocation_kind")?;
    let run_status: String = row.try_get("", "run_status")?;
    let revision = load_revision(connection, &revision_id).await?;
    let node = revision
        .definition
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| StorageError::Integrity("timed out node missing".into()))?;
    let error = canonical::to_vec(&json!({
        "schemaVersion":1,
        "code":"node_timeout",
        "safeMessage":"node execution deadline exceeded",
        "retryClass":"policy"
    }))?;
    let error_id = put_inline_object(connection, &error, now).await?;
    let updated = connection.execute_raw(sql(
        "UPDATE node_attempts SET status = 'timed_out', error_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status IN ('leased','running')",
        vec![error_id.clone().into(), now.into(), attempt_id.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Ok(());
    }
    connection.execute_raw(sql(
        "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE run_id = ? AND node_id = ? AND kind = 'attempt_ready' AND status = 'claimed'",
        vec![run_id.clone().into(), node_id.clone().into()],
    )).await?;
    add_object_ref(
        connection,
        &error_id,
        "node_attempt",
        attempt_id,
        "error",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id: &run_id,
            event_type: "node.attempt.timed_out",
            importance: "critical",
            node_instance_id: Some(&instance_id),
            attempt_id: Some(attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":node_id}),
            now,
        },
    )
    .await?;
    let next_ordinal = if invocation_kind == "retry" {
        current_ordinal + 1
    } else {
        0
    };
    let retry = node.retry_policy.as_ref().is_some_and(|policy| {
        policy.retry_on.iter().any(|code| code == "node_timeout")
            && next_ordinal >= 0
            && (next_ordinal as u64) < policy.max_retries
            && (attempt_no as u64) < revision.definition.limits.max_attempts_per_activation
    });
    if retry {
        schedule_retry(
            connection,
            &run_id,
            &instance_id,
            attempt_id,
            attempt_no + 1,
            next_ordinal,
            node,
            now,
        )
        .await?;
        connection.execute_raw(sql(
            "UPDATE node_instances SET status = 'waiting', updated_at = ? WHERE id = ? AND status IN ('ready','running')",
            vec![now.into(), instance_id.clone().into()],
        )).await?;
        if run_status == "interrupting" {
            finish_interrupt_if_drained(connection, &run_id, now).await?;
        } else if run_status == "running" {
            mark_run_waiting_if_idle(connection, &run_id, now).await?;
        }
    } else {
        connection.execute_raw(sql(
            "UPDATE node_instances SET status = 'failed', updated_at = ? WHERE id = ? AND status IN ('ready','running')",
            vec![now.into(), instance_id.into()],
        )).await?;
        fail_run(
            connection,
            &run_id,
            "node_timeout",
            "node execution deadline exceeded",
            now,
        )
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn schedule_retry<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    instance_id: &str,
    attempt_id: &str,
    next_attempt_no: i64,
    retry_ordinal: i64,
    node: &zhuangsheng_core::graph::GraphNode,
    now: i64,
) -> StorageResult<()> {
    let policy = node
        .retry_policy
        .as_ref()
        .ok_or_else(|| StorageError::Integrity("retry policy missing".into()))?;
    let delay = retry_delay_ms(policy, instance_id, retry_ordinal as u64);
    let due_at = now.saturating_add(i64::try_from(delay).unwrap_or(i64::MAX));
    let payload = canonical::to_vec(&json!({
        "schemaVersion":1,
        "nextAttemptNo":next_attempt_no,
        "retryOrdinal":retry_ordinal
    }))?;
    let payload_id = put_inline_object(connection, &payload, now).await?;
    let timer_id = new_id("timer");
    connection.execute_raw(sql(
        "INSERT INTO runtime_timers (id, run_id, node_instance_id, node_attempt_id, kind, due_at, dedupe_key, status, payload_object_id, created_at) VALUES (?, ?, ?, ?, 'retry', ?, ?, 'pending', ?, ?)",
        vec![timer_id.clone().into(), run_id.into(), instance_id.into(), attempt_id.into(), due_at.into(), format!("retry:{instance_id}:{retry_ordinal}").into(), payload_id.clone().into(), now.into()],
    )).await?;
    add_object_ref(
        connection,
        &payload_id,
        "runtime_timer",
        &timer_id,
        "payload",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id,
            event_type: "node.retry.scheduled",
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: Some(attempt_id),
            payload: json!({"schemaVersion":1,"retryOrdinal":retry_ordinal,"dueAt":due_at}),
            now,
        },
    )
    .await?;
    Ok(())
}

async fn fire_retry<C: ConnectionTrait>(
    connection: &C,
    timer_id: &str,
    run_id: &str,
    instance_id: &str,
    payload_id: &str,
    now: i64,
) -> StorageResult<()> {
    let status = connection.query_one_raw(sql(
        "SELECT status, control_epoch, execution_manifest_object_id FROM graph_runs WHERE id = ?",
        vec![run_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("retry run missing".into()))?;
    let run_status: String = status.try_get("", "status")?;
    if matches!(run_status.as_str(), "interrupted" | "interrupting") {
        connection.execute_raw(sql(
            "UPDATE runtime_timers SET status = 'ready', fired_at = ? WHERE id = ? AND status = 'pending'",
            vec![now.into(), timer_id.into()],
        )).await?;
        return Ok(());
    }
    if !matches!(run_status.as_str(), "running" | "waiting") {
        cancel_timer(connection, timer_id).await?;
        return Ok(());
    }
    let payload: Value = load_object_json(connection, payload_id).await?;
    let attempt_no = payload
        .get("nextAttemptNo")
        .and_then(Value::as_i64)
        .ok_or_else(|| StorageError::Integrity("retry attempt number missing".into()))?;
    let retry_ordinal = payload
        .get("retryOrdinal")
        .and_then(Value::as_i64)
        .ok_or_else(|| StorageError::Integrity("retry ordinal missing".into()))?;
    let source = connection
        .query_one_raw(sql(
            "SELECT node_attempt_id FROM runtime_timers WHERE id = ?",
            vec![timer_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("retry timer missing".into()))?;
    let source_attempt_id: String = source.try_get("", "node_attempt_id")?;
    if !mark_fired(connection, timer_id, now).await? {
        return Ok(());
    }
    let node = connection
        .query_one_raw(sql(
            "SELECT node_id FROM node_instances WHERE id = ? AND run_id = ? AND status = 'waiting'",
            vec![instance_id.into(), run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("retry node instance not waiting".into()))?;
    let node_id: String = node.try_get("", "node_id")?;
    let attempt_id = new_id("attempt");
    let inserted = connection.execute_raw(sql(
        "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) SELECT ?, ?, ?, ?, 'retry', 'queued', control_epoch, 0, ?, execution_manifest_object_id FROM graph_runs WHERE id = ? AND status IN ('running','waiting')",
        vec![attempt_id.clone().into(), instance_id.into(), attempt_no.into(), retry_ordinal.into(), format!("attempt:{instance_id}:{attempt_no}").into(), run_id.into()],
    )).await?;
    if inserted.rows_affected() != 1 {
        return Err(StorageError::Conflict("retry_run_status"));
    }
    copy_attempt_reads(connection, &source_attempt_id, &attempt_id, now).await?;
    connection.execute_raw(sql(
        "UPDATE node_instances SET status = 'ready', updated_at = ? WHERE id = ? AND status = 'waiting'",
        vec![now.into(), instance_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET total_attempts = total_attempts + 1 WHERE run_id = ?",
        vec![run_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE graph_runs SET status = 'running', updated_at = ? WHERE id = ? AND status = 'waiting'",
        vec![now.into(), run_id.into()],
    )).await?;
    let seq = append_event(
        connection,
        Event {
            run_id,
            event_type: "node.retry.ready",
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: Some(&attempt_id),
            payload: json!({"schemaVersion":1,"retryOrdinal":retry_ordinal}),
            now,
        },
    )
    .await?;
    enqueue_wakeup(
        connection,
        run_id,
        Some(&node_id),
        "attempt_ready",
        seq,
        &format!("attempt-ready:{attempt_id}"),
        now,
    )
    .await?;
    Ok(())
}

async fn finish_interrupt_if_drained<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let draining = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM node_attempts WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('leased','running') LIMIT 1",
        vec![run_id.into()],
    )).await?.is_some();
    if !draining {
        let updated = connection.execute_raw(sql(
            "UPDATE graph_runs SET status = 'interrupted', drain_epoch = NULL, updated_at = ? WHERE id = ? AND status = 'interrupting'",
            vec![now.into(), run_id.into()],
        )).await?;
        if updated.rows_affected() == 1 {
            append_event(
                connection,
                Event {
                    run_id,
                    event_type: "run.interrupted",
                    importance: "critical",
                    node_instance_id: None,
                    attempt_id: None,
                    payload: json!({"schemaVersion":1,"reason":"attempt_timeout"}),
                    now,
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn mark_run_waiting_if_idle<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let active = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM node_instances WHERE run_id = ? AND status IN ('ready','running') LIMIT 1",
        vec![run_id.into()],
    )).await?.is_some();
    let wakeup = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM scheduler_wakeups WHERE run_id = ? AND status IN ('pending','claimed') LIMIT 1",
        vec![run_id.into()],
    )).await?.is_some();
    if !active && !wakeup {
        let updated = connection.execute_raw(sql(
            "UPDATE graph_runs SET status = 'waiting', updated_at = ? WHERE id = ? AND status = 'running'",
            vec![now.into(), run_id.into()],
        )).await?;
        if updated.rows_affected() == 1 {
            append_event(
                connection,
                Event {
                    run_id,
                    event_type: "run.waiting",
                    importance: "critical",
                    node_instance_id: None,
                    attempt_id: None,
                    payload: json!({"schemaVersion":1,"reason":"retry_backoff"}),
                    now,
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn mark_fired<C: ConnectionTrait>(
    connection: &C,
    timer_id: &str,
    now: i64,
) -> StorageResult<bool> {
    Ok(connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'fired', fired_at = ? WHERE id = ? AND status = 'pending'",
        vec![now.into(), timer_id.into()],
    )).await?.rows_affected() == 1)
}

async fn cancel_timer<C: ConnectionTrait>(connection: &C, timer_id: &str) -> StorageResult<()> {
    connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE id = ? AND status IN ('pending','ready')",
        vec![timer_id.into()],
    )).await?;
    Ok(())
}
