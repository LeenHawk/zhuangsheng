use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{canonical, scheduler::FinalizeAttemptCommand};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::{
    attempt_state::AttemptState,
    events::{Event, add_object_ref, append_event, fail_run, finish_wakeup},
};

pub(super) async fn settle_interrupt_after_attempt<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let draining = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM node_attempts WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('leased','running') LIMIT 1",
        vec![run_id.into()],
    )).await?.is_some();
    if draining {
        return Ok(());
    }
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
                payload: json!({"schemaVersion":1}),
                now,
            },
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn complete_rows<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    final_outputs: &str,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute_raw(sql(
        "UPDATE node_attempts SET status = 'completed', result_idempotency_key = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status = 'running' AND worker_id = ? AND lease_fence = ? AND run_control_epoch = ?",
        vec![command.result_idempotency_key.clone().into(), now.into(), command.attempt_id.clone().into(), command.worker_id.clone().into(), (command.lease_fence as i64).into(), (command.run_control_epoch as i64).into()],
    )).await?;
    if attempt.rows_affected() != 1 {
        return Err(StorageError::Conflict("attempt_fence"));
    }
    connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
        vec![command.attempt_id.clone().into()],
    )).await?;
    let node = connection.execute_raw(sql(
        "UPDATE node_instances SET status = 'completed', final_outputs_object_id = ?, updated_at = ? WHERE id = ? AND status = 'running'",
        vec![final_outputs.into(), now.into(), state.node_instance_id.clone().into()],
    )).await?;
    if node.rows_affected() != 1 {
        return Err(StorageError::Conflict("node_instance_status"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn fail_attempt<C: ConnectionTrait>(
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
    connection.execute_raw(sql(
        "UPDATE node_attempts SET status = 'failed', result_idempotency_key = ?, error_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND status = 'running' AND worker_id = ? AND lease_fence = ?",
        vec![command.result_idempotency_key.clone().into(), error_id.clone().into(), now.into(), command.attempt_id.clone().into(), command.worker_id.clone().into(), (command.lease_fence as i64).into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
        vec![command.attempt_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
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
    append_event(connection, Event {
        run_id: &state.run_id, event_type: "node.failed", importance: "critical",
        node_instance_id: Some(&state.node_instance_id), attempt_id: Some(&command.attempt_id),
        payload: json!({"schemaVersion":1,"nodeId":state.node_id,"code":code,"safeMessage":message}), now,
    }).await?;
    fail_run(connection, &state.run_id, code, message, now).await?;
    finish_wakeup(connection, &command.wakeup_id).await?;
    Ok(())
}
