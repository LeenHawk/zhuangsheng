use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::scheduler::{ClaimedAttempt, SchedulerWork};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{apply::load_revision, helpers::sql},
};

use super::{
    events::{Event, append_event, finish_wakeup},
    load::load_inputs,
    read_set::load_router_memory,
    router::load_control_snapshot,
};

impl SqliteStore {
    pub(crate) async fn claim_next_work(
        &self,
        worker_id: &str,
        now: i64,
        lease_until: i64,
    ) -> StorageResult<Option<SchedulerWork>> {
        let transaction = self.db.begin().await?;
        let row = transaction
            .query_one(sql(
                "SELECT w.id, w.run_id, w.node_id, w.kind FROM scheduler_wakeups w JOIN graph_runs r ON r.id = w.run_id WHERE w.status = 'pending' AND w.available_at <= ? AND r.status = 'running' ORDER BY w.available_at, w.created_at, w.id LIMIT 1",
                vec![now.into()],
            ))
            .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let wakeup_id: String = row.try_get("", "id")?;
        let run_id: String = row.try_get("", "run_id")?;
        let node_id: Option<String> = row.try_get("", "node_id")?;
        let kind: String = row.try_get("", "kind")?;
        let claimed = transaction
            .execute(sql(
                "UPDATE scheduler_wakeups SET status = 'claimed', claimed_by = ?, lease_until = ? WHERE id = ? AND status = 'pending'",
                vec![worker_id.into(), lease_until.into(), wakeup_id.clone().into()],
            ))
            .await?;
        if claimed.rows_affected() != 1 {
            transaction.commit().await?;
            return Ok(None);
        }
        let work = match kind.as_str() {
            "attempt_ready" => {
                let Some(node_id) = node_id else {
                    return Err(StorageError::Integrity("attempt wakeup has no node".into()));
                };
                claim_attempt(
                    &transaction,
                    &wakeup_id,
                    &run_id,
                    &node_id,
                    worker_id,
                    now,
                    lease_until,
                )
                .await?
            }
            "node_maybe_ready" => node_id.map(|node_id| SchedulerWork::Activate {
                wakeup_id: wakeup_id.clone(),
                run_id: run_id.clone(),
                node_id,
            }),
            "settle_run" => Some(SchedulerWork::Settle {
                wakeup_id: wakeup_id.clone(),
                run_id: run_id.clone(),
            }),
            _ => {
                return Err(StorageError::Integrity(
                    "unknown scheduler wakeup kind".into(),
                ));
            }
        };
        let work = if work.is_none() {
            finish_wakeup(&transaction, &wakeup_id).await?;
            Some(SchedulerWork::Noop)
        } else {
            work
        };
        transaction.commit().await?;
        Ok(work)
    }

    pub(crate) async fn mark_attempt_running(
        &self,
        attempt: &ClaimedAttempt,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let updated = transaction.execute(sql(
            "UPDATE node_attempts SET status = 'running', started_at = ? WHERE id = ? AND status = 'leased' AND worker_id = ? AND lease_fence = ? AND run_control_epoch = ?",
            vec![now.into(), attempt.attempt_id.clone().into(), attempt.worker_id.clone().into(), (attempt.lease_fence as i64).into(), (attempt.run_control_epoch as i64).into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("attempt_lease"));
        }
        let node = transaction.execute(sql(
            "UPDATE node_instances SET status = 'running', updated_at = ? WHERE id = ? AND status = 'ready'",
            vec![now.into(), attempt.node_instance_id.clone().into()],
        )).await?;
        if node.rows_affected() != 1 {
            return Err(StorageError::Conflict("node_instance_status"));
        }
        append_event(
            &transaction,
            Event {
                run_id: &attempt.run_id,
                event_type: "node.started",
                importance: "critical",
                node_instance_id: Some(&attempt.node_instance_id),
                attempt_id: Some(&attempt.attempt_id),
                payload: json!({"schemaVersion":1,"nodeId":attempt.node.id,"leaseFence":attempt.lease_fence}),
                now,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn claim_attempt<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: &str,
    run_id: &str,
    node_id: &str,
    worker_id: &str,
    now: i64,
    requested_lease_until: i64,
) -> StorageResult<Option<SchedulerWork>> {
    let row = connection.query_one(sql(
        "SELECT a.id AS attempt_id, a.lease_fence, ni.id AS node_instance_id, ni.graph_revision_id, ni.inputs_object_id, r.control_epoch, r.deadline_at FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE ni.run_id = ? AND ni.node_id = ? AND ni.status = 'ready' AND a.status = 'queued' ORDER BY a.attempt_no LIMIT 1",
        vec![run_id.into(), node_id.into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    let attempt_id: String = row.try_get("", "attempt_id")?;
    let instance_id: String = row.try_get("", "node_instance_id")?;
    let revision_id: String = row.try_get("", "graph_revision_id")?;
    let inputs_id: String = row.try_get("", "inputs_object_id")?;
    let control_epoch: i64 = row.try_get("", "control_epoch")?;
    let run_deadline: i64 = row.try_get("", "deadline_at")?;
    let old_fence: i64 = row.try_get("", "lease_fence")?;
    let revision = load_revision(connection, &revision_id).await?;
    let node = revision
        .definition
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .cloned()
        .ok_or_else(|| StorageError::Integrity("run node missing from revision".into()))?;
    let deadline = node
        .timeout_ms
        .and_then(|timeout| i64::try_from(timeout).ok())
        .map_or(run_deadline, |timeout| {
            now.saturating_add(timeout).min(run_deadline)
        });
    let lease_until = requested_lease_until.min(deadline);
    let updated = connection.execute(sql(
        "UPDATE node_attempts SET status = 'leased', run_control_epoch = ?, lease_fence = lease_fence + 1, worker_id = ?, lease_until = ?, deadline_at = ? WHERE id = ? AND status = 'queued' AND lease_fence = ?",
        vec![control_epoch.into(), worker_id.into(), lease_until.into(), deadline.into(), attempt_id.clone().into(), old_fence.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Ok(None);
    }
    connection.execute(sql(
        "INSERT OR IGNORE INTO runtime_timers (id, run_id, node_instance_id, node_attempt_id, kind, due_at, dedupe_key, status, created_at) VALUES (?, ?, ?, ?, 'attempt_deadline', ?, ?, 'pending', ?)",
        vec![crate::graph::helpers::new_id("timer").into(), run_id.into(), instance_id.clone().into(), attempt_id.clone().into(), deadline.into(), format!("attempt-deadline:{attempt_id}").into(), now.into()],
    )).await?;
    let fence = u64::try_from(old_fence + 1)
        .map_err(|_| StorageError::Integrity("invalid lease fence".into()))?;
    append_event(
        connection,
        Event {
            run_id,
            event_type: "node.attempt.leased",
            importance: "info",
            node_instance_id: Some(&instance_id),
            attempt_id: Some(&attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":node_id,"leaseFence":fence}),
            now,
        },
    )
    .await?;
    Ok(Some(SchedulerWork::Attempt(Box::new(ClaimedAttempt {
        wakeup_id: wakeup_id.into(),
        run_id: run_id.into(),
        node_instance_id: instance_id.clone(),
        attempt_id: attempt_id.clone(),
        worker_id: worker_id.into(),
        lease_fence: fence,
        run_control_epoch: u64::try_from(control_epoch)
            .map_err(|_| StorageError::Integrity("invalid run control epoch".into()))?,
        inputs: load_inputs(connection, &node, &inputs_id).await?,
        memory: load_router_memory(connection, &attempt_id, &node).await?,
        router_control: load_control_snapshot(connection, &node, &instance_id).await?,
        node,
    }))))
}
