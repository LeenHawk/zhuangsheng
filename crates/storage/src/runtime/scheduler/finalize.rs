use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    router::RouterDecisionError,
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
    attempt_state::{AttemptState, load_attempt, validate_fence},
    emit::{commit_run_output, emit_edges, ensure_edge_capacity, prepare_outputs},
    events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run, finish_wakeup},
    reconcile::{ReconcileAttempt, ReconcileOutcome, reconcile_if_stale},
    router::{persist_decision, persist_error},
};

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
        if matches!(
            &command.result,
            BuiltinResult::RouterDecision { .. } | BuiltinResult::RouterFailed { .. }
        ) {
            match reconcile_if_stale(
                &transaction,
                ReconcileAttempt {
                    run_id: &state.run_id,
                    node_instance_id: &state.node_instance_id,
                    attempt_id: &command.attempt_id,
                    wakeup_id: &command.wakeup_id,
                    worker_id: &command.worker_id,
                    lease_fence: command.lease_fence,
                    run_control_epoch: command.run_control_epoch,
                    result_idempotency_key: &command.result_idempotency_key,
                },
                node,
                &revision.definition.limits,
                now,
            )
            .await?
            {
                ReconcileOutcome::Continue => {}
                ReconcileOutcome::Requeued => {
                    transaction.commit().await?;
                    return Ok(());
                }
                ReconcileOutcome::Exhausted => {
                    let error = RouterDecisionError {
                        code: "router_read_conflict_exhausted".into(),
                        safe_message: "Router read reconciliation budget was exhausted".into(),
                        rule_id: None,
                        evaluated_rule_ids: Vec::new(),
                    };
                    persist_error(
                        &transaction,
                        &state.run_id,
                        &state.node_instance_id,
                        &command.attempt_id,
                        &error,
                        now,
                    )
                    .await?;
                    fail_attempt(
                        &transaction,
                        &state,
                        &command,
                        &error.code,
                        &error.safe_message,
                        now,
                    )
                    .await?;
                    transaction.commit().await?;
                    return Ok(());
                }
            }
        }
        let router_outputs: BTreeMap<String, Value>;
        let (outputs, router_decision) = match &command.result {
            BuiltinResult::Failed { code, safe_message } => {
                fail_attempt(&transaction, &state, &command, code, safe_message, now).await?;
                transaction.commit().await?;
                return Ok(());
            }
            BuiltinResult::RouterFailed { error } => {
                persist_error(
                    &transaction,
                    &state.run_id,
                    &state.node_instance_id,
                    &command.attempt_id,
                    error,
                    now,
                )
                .await?;
                fail_attempt(
                    &transaction,
                    &state,
                    &command,
                    &error.code,
                    &error.safe_message,
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Ok(());
            }
            BuiltinResult::Completed { outputs } => (outputs, None),
            BuiltinResult::RouterDecision { decision } => {
                router_outputs = decision
                    .selected_ports
                    .iter()
                    .map(|port| (port.clone(), decision.payload.clone()))
                    .collect();
                (&router_outputs, Some(decision))
            }
        };
        let output_order = router_decision.map(|decision| decision.selected_ports.as_slice());
        let stored = match prepare_outputs(
            &transaction,
            node,
            outputs,
            output_order,
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
        if let Some(decision) = router_decision {
            if let Err(error) = ensure_edge_capacity(
                &transaction,
                &state.run_id,
                node,
                &revision.definition,
                &stored,
                &revision.definition.limits,
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
            persist_decision(
                &transaction,
                &state.run_id,
                &state.node_instance_id,
                &command.attempt_id,
                decision,
                &stored,
                now,
            )
            .await?;
        }
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
            output_order,
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
        transaction.commit().await?;
        Ok(())
    }
}

async fn settle_interrupt_after_attempt<C: ConnectionTrait>(
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
