use std::collections::BTreeMap;

use sea_orm::TransactionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    router::RouterDecisionError,
    scheduler::{BuiltinResult, FinalizeAttemptCommand},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{apply::load_revision, helpers::put_inline_object},
};

use super::{
    attempt_finish::{complete_rows, fail_attempt, settle_interrupt_after_attempt},
    attempt_state::{load_attempt, validate_fence},
    emit::{commit_run_output, emit_edges, ensure_edge_capacity, prepare_outputs},
    events::{Event, add_object_ref, append_event, enqueue_wakeup, finish_wakeup},
    expand_finalize,
    reconcile::{ReconcileAttempt, ReconcileOutcome, reconcile_if_stale},
    router::{persist_decision, persist_error},
    static_writes,
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
        if let BuiltinResult::Expanded { output, values } = &command.result {
            expand_finalize::finalize(
                &transaction,
                &state,
                &command,
                node,
                &revision.definition,
                output,
                values,
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
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
            BuiltinResult::Expanded { .. } => unreachable!("handled above"),
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
        if let Err(error) = static_writes::apply(
            &transaction,
            &state,
            &command.attempt_id,
            node,
            &stored,
            now,
        )
        .await
        {
            if !matches!(
                error,
                StorageError::InputContract(_)
                    | StorageError::InvalidArgument(_)
                    | StorageError::Conflict("state_conflict" | "context_patch_base")
                    | StorageError::StatePatch(_)
            ) {
                return Err(error);
            }
            let code = if matches!(&error, StorageError::Conflict("state_conflict")) {
                "state_conflict"
            } else {
                "static_context_write_failed"
            };
            fail_attempt(
                &transaction,
                &state,
                &command,
                code,
                &error.to_string(),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
        if let Some(decision) = router_decision {
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

fn is_contract_error(error: &StorageError) -> bool {
    matches!(
        error,
        StorageError::InputContract(_) | StorageError::Domain(_)
    )
}
