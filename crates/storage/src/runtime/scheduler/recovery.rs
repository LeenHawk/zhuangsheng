use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::canonical;

use crate::{
    SqliteStore, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{new_id, put_inline_object, sql},
    },
};

use super::events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run};

impl SqliteStore {
    pub(crate) async fn recover_expired_leases(&self, now: i64) -> StorageResult<u64> {
        let transaction = self.db.begin().await?;
        let reset = transaction.execute(sql(
            "UPDATE scheduler_wakeups SET status = 'pending', claimed_by = NULL, lease_until = NULL WHERE status = 'claimed' AND lease_until <= ?",
            vec![now.into()],
        )).await?.rows_affected();
        let row = transaction.query_one(sql(
            "SELECT a.id AS attempt_id, a.attempt_no, a.retry_ordinal, a.executor_object_id, ni.id AS node_instance_id, ni.run_id, ni.node_id, ni.graph_revision_id, r.status AS run_status FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE a.status IN ('leased','running') AND a.lease_until <= ? AND r.status IN ('running','interrupting') ORDER BY a.lease_until, a.id LIMIT 1",
            vec![now.into()],
        )).await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(reset);
        };
        let attempt_id: String = row.try_get("", "attempt_id")?;
        let attempt_no: i64 = row.try_get("", "attempt_no")?;
        let retry_ordinal: i64 = row.try_get("", "retry_ordinal")?;
        let executor_id: String = row.try_get("", "executor_object_id")?;
        let instance_id: String = row.try_get("", "node_instance_id")?;
        let run_id: String = row.try_get("", "run_id")?;
        let node_id: String = row.try_get("", "node_id")?;
        let revision_id: String = row.try_get("", "graph_revision_id")?;
        let run_status: String = row.try_get("", "run_status")?;
        let revision = load_revision(&transaction, &revision_id).await?;
        if attempt_no as u64 >= revision.definition.limits.max_attempts_per_activation {
            fail_run(
                &transaction,
                &run_id,
                "attempt_limit_exceeded",
                "attempt lease expired after retry budget was exhausted",
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(reset + 1);
        }
        let error = canonical::to_vec(&json!({
            "schemaVersion":1,
            "code":"node_lease_expired",
            "safeMessage":"worker lease expired",
            "retryClass":"policy"
        }))?;
        let error_id = put_inline_object(&transaction, &error, now).await?;
        let expired = transaction.execute(sql(
            "UPDATE node_attempts SET status = 'failed', error_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status IN ('leased','running')",
            vec![error_id.clone().into(), now.into(), attempt_id.clone().into()],
        )).await?;
        if expired.rows_affected() != 1 {
            transaction.commit().await?;
            return Ok(reset);
        }
        transaction.execute(sql(
            "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
            vec![attempt_id.clone().into()],
        )).await?;
        transaction.execute(sql(
            "UPDATE node_instances SET status = 'ready', updated_at = ? WHERE id = ? AND status IN ('ready','running')",
            vec![now.into(), instance_id.clone().into()],
        )).await?;
        let next_attempt = new_id("attempt");
        transaction.execute(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) SELECT ?, ?, ?, ?, 'retry', 'queued', control_epoch, 0, ?, ? FROM graph_runs WHERE id = ? AND status IN ('running','interrupting')",
            vec![next_attempt.clone().into(), instance_id.clone().into(), (attempt_no + 1).into(), (retry_ordinal + 1).into(), format!("attempt:{instance_id}:{}", attempt_no + 1).into(), executor_id.into(), run_id.clone().into()],
        )).await?;
        transaction.execute(sql(
            "UPDATE run_execution_counters SET total_attempts = total_attempts + 1 WHERE run_id = ?",
            vec![run_id.clone().into()],
        )).await?;
        add_object_ref(
            &transaction,
            &error_id,
            "node_attempt",
            &attempt_id,
            "error",
            now,
        )
        .await?;
        let seq = append_event(&transaction, Event {
            run_id: &run_id,
            event_type: "node.lease.expired",
            importance: "critical",
            node_instance_id: Some(&instance_id),
            attempt_id: Some(&attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":node_id,"replacementAttemptId":next_attempt}),
            now,
        }).await?;
        enqueue_wakeup(
            &transaction,
            &run_id,
            Some(&node_id),
            "attempt_ready",
            seq,
            &format!("attempt-ready:{next_attempt}"),
            now,
        )
        .await?;
        let other_draining = transaction.query_one(sql(
            "SELECT 1 AS present FROM node_attempts WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('leased','running') LIMIT 1",
            vec![run_id.clone().into()],
        )).await?.is_some();
        if run_status == "interrupting" && !other_draining {
            transaction.execute(sql(
                "UPDATE graph_runs SET status = 'interrupted', drain_epoch = NULL, updated_at = ? WHERE id = ? AND status = 'interrupting'",
                vec![now.into(), run_id.clone().into()],
            )).await?;
            append_event(
                &transaction,
                Event {
                    run_id: &run_id,
                    event_type: "run.interrupted",
                    importance: "critical",
                    node_instance_id: None,
                    attempt_id: None,
                    payload: json!({"schemaVersion":1,"reason":"lease_expired"}),
                    now,
                },
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(reset + 1)
    }
}
