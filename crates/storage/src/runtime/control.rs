use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    runtime::{RunControlCommand, RunView},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, now_ms, put_inline_object, sql},
    llm::fence_run_effects,
};

use super::{
    query::load_run,
    scheduler::{Event, add_object_ref, append_event, enqueue_wakeup},
};

#[derive(Clone, Copy)]
enum ControlKind {
    Interrupt,
    Resume,
    Cancel,
}

impl ControlKind {
    fn name(self) -> &'static str {
        match self {
            Self::Interrupt => "interrupt",
            Self::Resume => "resume",
            Self::Cancel => "cancel",
        }
    }
}

impl SqliteStore {
    pub async fn request_interrupt(&self, command: RunControlCommand) -> StorageResult<RunView> {
        self.apply_control(command, ControlKind::Interrupt).await
    }

    pub async fn resume_interrupted(&self, command: RunControlCommand) -> StorageResult<RunView> {
        self.apply_control(command, ControlKind::Resume).await
    }

    pub async fn request_cancel(&self, command: RunControlCommand) -> StorageResult<RunView> {
        self.apply_control(command, ControlKind::Cancel).await
    }

    async fn apply_control(
        &self,
        command: RunControlCommand,
        kind: ControlKind,
    ) -> StorageResult<RunView> {
        validate_command(&command)?;
        let digest = canonical::hash(&json!({
            "command": kind.name(),
            "runId": command.run_id,
            "expectedEpoch": command.expected_epoch,
            "reason": command.reason,
        }))?;
        let now = now_ms();
        let transaction = self.db.begin().await?;
        if let Some(result) = replay_control(
            &transaction,
            &command.run_id,
            &command.idempotency_key,
            &digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(result);
        }
        let current = load_run(&transaction, &command.run_id).await?;
        if current.control_epoch != command.expected_epoch {
            return Err(StorageError::Conflict("run_control_epoch"));
        }
        let command_id = new_id("runcommand");
        let payload = canonical::to_vec(&json!({
            "schemaVersion":1,
            "reason":command.reason,
        }))?;
        let payload_id = put_inline_object(&transaction, &payload, now).await?;
        transaction.execute(sql(
            "INSERT INTO run_commands (id, run_id, command_kind, idempotency_key, request_digest, expected_control_epoch, payload_object_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
            vec![command_id.clone().into(), command.run_id.clone().into(), kind.name().into(), command.idempotency_key.clone().into(), digest.into(), (command.expected_epoch as i64).into(), payload_id.clone().into(), now.into()],
        )).await?;
        match kind {
            ControlKind::Interrupt => {
                apply_interrupt(&transaction, &command, &command_id, now).await?
            }
            ControlKind::Resume => apply_resume(&transaction, &command, &command_id, now).await?,
            ControlKind::Cancel => apply_cancel(&transaction, &command, now).await?,
        }
        let result = load_run(&transaction, &command.run_id).await?;
        let result_id = put_inline_object(&transaction, &canonical::to_vec(&result)?, now).await?;
        transaction.execute(sql(
            "UPDATE run_commands SET status = 'completed', result_object_id = ?, applied_at = ? WHERE id = ? AND status = 'pending'",
            vec![result_id.clone().into(), now.into(), command_id.clone().into()],
        )).await?;
        add_object_ref(
            &transaction,
            &payload_id,
            "run_command",
            &command_id,
            "payload",
            now,
        )
        .await?;
        add_object_ref(
            &transaction,
            &result_id,
            "run_command",
            &command_id,
            "result",
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

async fn apply_interrupt<C: ConnectionTrait>(
    connection: &C,
    command: &RunControlCommand,
    command_id: &str,
    now: i64,
) -> StorageResult<()> {
    let status = run_status(connection, &command.run_id).await?;
    if !matches!(status.as_str(), "running" | "waiting") {
        return Err(StorageError::Conflict("run_lifecycle"));
    }
    let draining = connection.query_one(sql(
        "SELECT 1 AS present FROM node_attempts WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('leased','running') LIMIT 1",
        vec![command.run_id.clone().into()],
    )).await?.is_some();
    let next_status = if draining {
        "interrupting"
    } else {
        "interrupted"
    };
    let drain_epoch: Option<i64> = draining.then_some(command.expected_epoch as i64);
    let updated = connection.execute(sql(
        "UPDATE graph_runs SET status = ?, control_epoch = control_epoch + 1, drain_epoch = ?, updated_at = ? WHERE id = ? AND control_epoch = ? AND status IN ('running','waiting')",
        vec![next_status.into(), drain_epoch.into(), now.into(), command.run_id.clone().into(), (command.expected_epoch as i64).into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_control_epoch"));
    }
    append_event(
        connection,
        Event {
            run_id: &command.run_id,
            event_type: "run.interrupt.requested",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1,"commandId":command_id,"draining":draining}),
            now,
        },
    )
    .await?;
    if !draining {
        append_event(
            connection,
            Event {
                run_id: &command.run_id,
                event_type: "run.interrupted",
                importance: "critical",
                node_instance_id: None,
                attempt_id: None,
                payload: json!({"schemaVersion":1,"commandId":command_id}),
                now,
            },
        )
        .await?;
    }
    Ok(())
}

async fn apply_resume<C: ConnectionTrait>(
    connection: &C,
    command: &RunControlCommand,
    command_id: &str,
    now: i64,
) -> StorageResult<()> {
    let updated = connection.execute(sql(
        "UPDATE graph_runs SET status = 'running', control_epoch = control_epoch + 1, drain_epoch = NULL, updated_at = ? WHERE id = ? AND control_epoch = ? AND status = 'interrupted'",
        vec![now.into(), command.run_id.clone().into(), (command.expected_epoch as i64).into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_lifecycle"));
    }
    let seq = append_event(
        connection,
        Event {
            run_id: &command.run_id,
            event_type: "run.resumed",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1,"commandId":command_id}),
            now,
        },
    )
    .await?;
    connection.execute(sql(
        "UPDATE runtime_timers SET status = 'pending', due_at = ?, fired_at = NULL WHERE run_id = ? AND status = 'ready'",
        vec![now.into(), command.run_id.clone().into()],
    )).await?;
    restore_wakeups(connection, &command.run_id, command_id, seq, now).await?;
    enqueue_wakeup(
        connection,
        &command.run_id,
        None,
        "settle_run",
        seq,
        &format!("resume-settle:{command_id}"),
        now,
    )
    .await?;
    Ok(())
}

async fn apply_cancel<C: ConnectionTrait>(
    connection: &C,
    command: &RunControlCommand,
    now: i64,
) -> StorageResult<()> {
    let status = run_status(connection, &command.run_id).await?;
    if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(StorageError::Conflict("run_lifecycle"));
    }
    append_event(
        connection,
        Event {
            run_id: &command.run_id,
            event_type: "run.cancel.requested",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1}),
            now,
        },
    )
    .await?;
    let updated = connection.execute(sql(
        "UPDATE graph_runs SET status = 'cancelled', control_epoch = control_epoch + 1, drain_epoch = NULL, finished_at = ?, updated_at = ? WHERE id = ? AND control_epoch = ? AND status NOT IN ('completed','failed','cancelled')",
        vec![now.into(), now.into(), command.run_id.clone().into(), (command.expected_epoch as i64).into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_control_epoch"));
    }
    fence_run_effects(
        connection,
        &command.run_id,
        command.expected_epoch.saturating_add(1),
        now,
    )
    .await?;
    connection.execute(sql(
        "UPDATE node_attempts SET status = 'cancelled', worker_id = NULL, lease_until = NULL, finished_at = ? WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('queued','leased','running','waiting')",
        vec![now.into(), command.run_id.clone().into()],
    )).await?;
    connection.execute(sql(
        "UPDATE node_instances SET status = 'cancelled', updated_at = ? WHERE run_id = ? AND status IN ('ready','running','waiting')",
        vec![now.into(), command.run_id.clone().into()],
    )).await?;
    connection.execute(sql(
        "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE run_id = ? AND status IN ('pending','claimed')",
        vec![command.run_id.clone().into()],
    )).await?;
    connection.execute(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE run_id = ? AND status IN ('pending','ready')",
        vec![command.run_id.clone().into()],
    )).await?;
    connection.execute(sql(
        "UPDATE node_waits SET status = 'cancelled', resolved_at = ? WHERE run_id = ? AND status = 'open'",
        vec![now.into(), command.run_id.clone().into()],
    )).await?;
    connection
        .execute(sql(
            "UPDATE run_execution_counters SET open_waits = 0 WHERE run_id = ?",
            vec![command.run_id.clone().into()],
        ))
        .await?;
    append_event(
        connection,
        Event {
            run_id: &command.run_id,
            event_type: "run.cancelled",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1}),
            now,
        },
    )
    .await?;
    Ok(())
}

async fn restore_wakeups<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    command_id: &str,
    caused_by_seq: i64,
    now: i64,
) -> StorageResult<()> {
    let rows = connection.query_all(sql(
        "SELECT a.id AS attempt_id, ni.node_id FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id WHERE ni.run_id = ? AND a.status = 'queued'",
        vec![run_id.into()],
    )).await?;
    for row in rows {
        let attempt_id: String = row.try_get("", "attempt_id")?;
        let node_id: String = row.try_get("", "node_id")?;
        enqueue_wakeup(
            connection,
            run_id,
            Some(&node_id),
            "attempt_ready",
            caused_by_seq,
            &format!("resume:{command_id}:{attempt_id}"),
            now,
        )
        .await?;
    }
    Ok(())
}

async fn replay_control<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<RunView>> {
    let row = connection.query_one(sql(
        "SELECT request_digest, result_object_id, status FROM run_commands WHERE run_id = ? AND idempotency_key = ?",
        vec![run_id.into(), key.into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    if row.try_get::<String>("", "status")? != "completed" {
        return Err(StorageError::Conflict("run_command_pending"));
    }
    let object_id: String = row.try_get("", "result_object_id")?;
    Ok(Some(load_object_json(connection, &object_id).await?))
}

async fn run_status<C: ConnectionTrait>(connection: &C, run_id: &str) -> StorageResult<String> {
    connection
        .query_one(sql(
            "SELECT status FROM graph_runs WHERE id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "graph_run",
            id: run_id.into(),
        })?
        .try_get("", "status")
        .map_err(Into::into)
}

fn validate_command(command: &RunControlCommand) -> StorageResult<()> {
    if command.idempotency_key.trim().is_empty()
        || command.idempotency_key.len() > 200
        || command
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > 500)
    {
        return Err(StorageError::InvalidArgument(
            "invalid run control command".into(),
        ));
    }
    Ok(())
}
