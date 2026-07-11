use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    scheduler::{BuiltinResult, FinalizeAttemptCommand},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{put_inline_object, sql},
    },
};

use super::{
    emit::{commit_run_output, emit_edges, prepare_outputs},
    events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run, finish_wakeup},
};

struct AttemptState {
    run_id: String,
    node_instance_id: String,
    node_id: String,
    graph_revision_id: String,
    inputs_object_id: String,
    status: String,
    worker_id: Option<String>,
    lease_fence: i64,
    run_control_epoch: i64,
    result_key: Option<String>,
    run_status: String,
    current_control_epoch: i64,
    drain_epoch: Option<i64>,
    lease_until: Option<i64>,
    attempt_deadline: Option<i64>,
    run_deadline: i64,
}

impl SqliteStore {
    pub(crate) async fn finalize_attempt(
        &self,
        command: FinalizeAttemptCommand,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let state = load_attempt(&transaction, &command.attempt_id).await?;
        if state.status == "completed"
            && state.result_key.as_deref() == Some(&command.result_idempotency_key)
        {
            transaction.commit().await?;
            return Ok(());
        }
        validate_fence(&state, &command, now)?;
        let revision = load_revision(&transaction, &state.graph_revision_id).await?;
        let node = revision
            .definition
            .nodes
            .iter()
            .find(|node| node.id == state.node_id)
            .ok_or_else(|| StorageError::Integrity("attempt node missing from revision".into()))?;
        match &command.result {
            BuiltinResult::Failed { code, safe_message } => {
                fail_attempt(&transaction, &state, &command, code, safe_message, now).await?;
            }
            BuiltinResult::Completed { outputs } => {
                let stored = match prepare_outputs(
                    &transaction,
                    node,
                    outputs,
                    &revision.definition.limits,
                    now,
                )
                .await
                {
                    Ok(stored) => stored,
                    Err(error) if is_contract_error(&error) => {
                        fail_attempt(
                            &transaction,
                            &state,
                            &command,
                            "node_output_contract_violation",
                            &error.to_string(),
                            now,
                        )
                        .await?;
                        transaction.commit().await?;
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                if let Err(error) = commit_run_output(
                    &transaction,
                    &state.run_id,
                    &state.node_instance_id,
                    &state.inputs_object_id,
                    node,
                    &revision.definition,
                    now,
                )
                .await
                {
                    if !is_contract_error(&error) {
                        return Err(error);
                    }
                    fail_attempt(
                        &transaction,
                        &state,
                        &command,
                        "run_output_contract_violation",
                        &error.to_string(),
                        now,
                    )
                    .await?;
                    transaction.commit().await?;
                    return Ok(());
                }
                let final_outputs = put_inline_object(
                    &transaction,
                    &canonical::to_vec(&json!({"schemaVersion":1,"outputs":stored}))?,
                    now,
                )
                .await?;
                if let Err(error) = emit_edges(
                    &transaction,
                    &state.run_id,
                    &state.node_instance_id,
                    node,
                    &revision.definition,
                    &stored,
                    &revision.definition.limits,
                    now,
                )
                .await
                {
                    if !is_contract_error(&error) {
                        return Err(error);
                    }
                    fail_attempt(
                        &transaction,
                        &state,
                        &command,
                        "run_limit_exceeded",
                        &error.to_string(),
                        now,
                    )
                    .await?;
                    transaction.commit().await?;
                    return Ok(());
                }
                complete_rows(&transaction, &state, &command, &final_outputs, now).await?;
                add_object_ref(
                    &transaction,
                    &final_outputs,
                    "node_instance",
                    &state.node_instance_id,
                    "final_outputs",
                    now,
                )
                .await?;
                let seq = append_event(
                    &transaction,
                    Event {
                        run_id: &state.run_id,
                        event_type: "node.completed",
                        importance: "critical",
                        node_instance_id: Some(&state.node_instance_id),
                        attempt_id: Some(&command.attempt_id),
                        payload: json!({"schemaVersion":1,"nodeId":state.node_id}),
                        now,
                    },
                )
                .await?;
                finish_wakeup(&transaction, &command.wakeup_id).await?;
                if !node.is_entry {
                    enqueue_wakeup(
                        &transaction,
                        &state.run_id,
                        Some(&state.node_id),
                        "node_maybe_ready",
                        seq,
                        &format!("recheck:{}", state.node_instance_id),
                        now,
                    )
                    .await?;
                }
                enqueue_wakeup(
                    &transaction,
                    &state.run_id,
                    None,
                    "settle_run",
                    seq,
                    &format!("settle:{}", state.node_instance_id),
                    now,
                )
                .await?;
                settle_interrupt_after_attempt(&transaction, &state.run_id, now).await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }
}

async fn load_attempt<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
) -> StorageResult<AttemptState> {
    let row = connection.query_one(sql(
        "SELECT a.status, a.worker_id, a.lease_fence, a.run_control_epoch, a.result_idempotency_key, a.lease_until, a.deadline_at AS attempt_deadline, ni.id AS node_instance_id, ni.run_id, ni.node_id, ni.graph_revision_id, ni.inputs_object_id, r.status AS run_status, r.control_epoch AS current_control_epoch, r.drain_epoch, r.deadline_at AS run_deadline FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE a.id = ?",
        vec![attempt_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "node_attempt", id: attempt_id.into() })?;
    Ok(AttemptState {
        run_id: row.try_get("", "run_id")?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        node_id: row.try_get("", "node_id")?,
        graph_revision_id: row.try_get("", "graph_revision_id")?,
        inputs_object_id: row.try_get("", "inputs_object_id")?,
        status: row.try_get("", "status")?,
        worker_id: row.try_get("", "worker_id")?,
        lease_fence: row.try_get("", "lease_fence")?,
        run_control_epoch: row.try_get("", "run_control_epoch")?,
        result_key: row.try_get("", "result_idempotency_key")?,
        run_status: row.try_get("", "run_status")?,
        current_control_epoch: row.try_get("", "current_control_epoch")?,
        drain_epoch: row.try_get("", "drain_epoch")?,
        lease_until: row.try_get("", "lease_until")?,
        attempt_deadline: row.try_get("", "attempt_deadline")?,
        run_deadline: row.try_get("", "run_deadline")?,
    })
}

fn validate_fence(
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    now: i64,
) -> StorageResult<()> {
    let lifecycle_accepts = (state.run_status == "running"
        && state.run_control_epoch == state.current_control_epoch)
        || (state.run_status == "interrupting"
            && state.drain_epoch == Some(state.run_control_epoch));
    if state.status != "running"
        || state.worker_id.as_deref() != Some(&command.worker_id)
        || state.lease_fence != command.lease_fence as i64
        || state.run_control_epoch != command.run_control_epoch as i64
        || !lifecycle_accepts
        || state.lease_until.is_none_or(|deadline| now >= deadline)
        || state
            .attempt_deadline
            .is_none_or(|deadline| now >= deadline)
        || now >= state.run_deadline
    {
        return Err(StorageError::Conflict("attempt_fence"));
    }
    Ok(())
}

async fn settle_interrupt_after_attempt<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let draining = connection.query_one(sql(
        "SELECT 1 AS present FROM node_attempts WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('leased','running') LIMIT 1",
        vec![run_id.into()],
    )).await?.is_some();
    if draining {
        return Ok(());
    }
    let updated = connection.execute(sql(
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
                payload: json!({"schemaVersion":1}),
                now,
            },
        )
        .await?;
    }
    Ok(())
}

fn is_contract_error(error: &StorageError) -> bool {
    matches!(
        error,
        StorageError::InputContract(_) | StorageError::Domain(_)
    )
}

async fn complete_rows<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    final_outputs: &str,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute(sql(
        "UPDATE node_attempts SET status = 'completed', result_idempotency_key = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status = 'running' AND worker_id = ? AND lease_fence = ? AND run_control_epoch = ?",
        vec![command.result_idempotency_key.clone().into(), now.into(), command.attempt_id.clone().into(), command.worker_id.clone().into(), (command.lease_fence as i64).into(), (command.run_control_epoch as i64).into()],
    )).await?;
    if attempt.rows_affected() != 1 {
        return Err(StorageError::Conflict("attempt_fence"));
    }
    connection.execute(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
        vec![command.attempt_id.clone().into()],
    )).await?;
    let node = connection.execute(sql(
        "UPDATE node_instances SET status = 'completed', final_outputs_object_id = ?, updated_at = ? WHERE id = ? AND status = 'running'",
        vec![final_outputs.into(), now.into(), state.node_instance_id.clone().into()],
    )).await?;
    if node.rows_affected() != 1 {
        return Err(StorageError::Conflict("node_instance_status"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn fail_attempt<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    code: &str,
    message: &str,
    now: i64,
) -> StorageResult<()> {
    let error = canonical::to_vec(
        &json!({"schemaVersion":1,"code":code,"safeMessage":message,"retryClass":"never"}),
    )?;
    let error_id = put_inline_object(connection, &error, now).await?;
    connection.execute(sql(
        "UPDATE node_attempts SET status = 'failed', result_idempotency_key = ?, error_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status = 'running' AND worker_id = ? AND lease_fence = ?",
        vec![command.result_idempotency_key.clone().into(), error_id.clone().into(), now.into(), command.attempt_id.clone().into(), command.worker_id.clone().into(), (command.lease_fence as i64).into()],
    )).await?;
    connection.execute(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
        vec![command.attempt_id.clone().into()],
    )).await?;
    connection.execute(sql(
        "UPDATE node_instances SET status = 'failed', updated_at = ? WHERE id = ? AND status = 'running'",
        vec![now.into(), state.node_instance_id.clone().into()],
    )).await?;
    add_object_ref(
        connection,
        &error_id,
        "node_attempt",
        &command.attempt_id,
        "error",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id: &state.run_id,
            event_type: "node.failed",
            importance: "critical",
            node_instance_id: Some(&state.node_instance_id),
            attempt_id: Some(&command.attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":state.node_id,"code":code,"safeMessage":message}),
            now,
        },
    )
    .await?;
    fail_run(connection, &state.run_id, code, message, now).await?;
    finish_wakeup(connection, &command.wakeup_id).await?;
    Ok(())
}
