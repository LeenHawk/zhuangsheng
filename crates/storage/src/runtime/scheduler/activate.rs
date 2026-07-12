use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::graph::DraftNodeKind;

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{apply::load_revision, helpers::*},
};

use super::{
    activation_failure::{ActivationFailure, fail_input_activation},
    activation_inputs::{build_inputs, queue_heads},
    aggregator,
    events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run, finish_wakeup},
    join_by_key::{self, JoinPreparation},
    llm_read_set::resolve_llm_reads,
    load::load_inputs,
    read_set::resolve_router_reads,
    router::create_control_snapshot,
};

impl SqliteStore {
    pub(crate) async fn activate_if_ready(
        &self,
        wakeup_id: &str,
        run_id: &str,
        node_id: &str,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        if !claimed_wakeup(&transaction, wakeup_id, run_id).await? {
            transaction.commit().await?;
            return Ok(());
        }
        let run = transaction
            .query_one_raw(sql(
                "SELECT graph_revision_id FROM graph_runs WHERE id = ? AND status = 'running'",
                vec![run_id.into()],
            ))
            .await?;
        let Some(run) = run else {
            finish_wakeup(&transaction, wakeup_id).await?;
            transaction.commit().await?;
            return Ok(());
        };
        let revision_id: String = run.try_get("", "graph_revision_id")?;
        let revision = load_revision(&transaction, &revision_id).await?;
        let node = revision
            .definition
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .ok_or_else(|| StorageError::Integrity("wakeup node missing from revision".into()))?;
        if matches!(&node.kind, DraftNodeKind::Input { .. }) {
            finish_and_settle(&transaction, wakeup_id, run_id, now).await?;
            transaction.commit().await?;
            return Ok(());
        }
        if matches!(&node.kind, DraftNodeKind::Aggregator { .. }) {
            aggregator::activate(
                &transaction,
                wakeup_id,
                run_id,
                node,
                &revision_id,
                &revision.definition,
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
        if has_active(&transaction, run_id, node_id).await? {
            finish_wakeup(&transaction, wakeup_id).await?;
            transaction.commit().await?;
            return Ok(());
        }
        let mut join_tuple = None;
        let mut activation_failure = None;
        let heads = if matches!(&node.kind, DraftNodeKind::JoinByKey { .. }) {
            match join_by_key::prepare(
                &transaction,
                run_id,
                node,
                &revision.definition.edges,
                &revision.definition.limits,
                now,
            )
            .await?
            {
                JoinPreparation::NotReady => {
                    finish_and_settle(&transaction, wakeup_id, run_id, now).await?;
                    transaction.commit().await?;
                    return Ok(());
                }
                JoinPreparation::LimitExceeded(message) => {
                    fail_run(&transaction, run_id, "run_limit_exceeded", &message, now).await?;
                    transaction.commit().await?;
                    return Ok(());
                }
                JoinPreparation::Invalid(invalid) => {
                    activation_failure = Some((invalid.code, invalid.safe_message));
                    vec![invalid.head]
                }
                JoinPreparation::Ready(tuple) => {
                    let heads = tuple.heads.clone();
                    join_tuple = Some(tuple);
                    heads
                }
            }
        } else {
            let Some(heads) =
                queue_heads(&transaction, run_id, node, &revision.definition.edges).await?
            else {
                finish_and_settle(&transaction, wakeup_id, run_id, now).await?;
                transaction.commit().await?;
                return Ok(());
            };
            heads
        };
        if limits_exceeded(&transaction, run_id, &revision.definition.limits).await? {
            fail_run(
                &transaction,
                run_id,
                "run_limit_exceeded",
                "activation limit exceeded",
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
        let instance_id = new_id("nodeinst");
        let attempt_id = new_id("attempt");
        let activation_seq = allocate_activation_seq(&transaction, run_id, node_id).await?;
        if let Some((code, message)) = activation_failure {
            fail_input_activation(
                &transaction,
                run_id,
                node,
                &revision_id,
                &heads,
                &instance_id,
                &attempt_id,
                activation_seq,
                ActivationFailure {
                    code,
                    safe_message: &message,
                },
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
        let inputs_id = match build_inputs(
            &transaction,
            node,
            &heads,
            &instance_id,
            join_tuple.as_ref().map(|tuple| &tuple.key),
            now,
        )
        .await
        {
            Ok(inputs_id) => inputs_id,
            Err(StorageError::InputContract(_) | StorageError::Domain(_)) => {
                fail_input_activation(
                    &transaction,
                    run_id,
                    node,
                    &revision_id,
                    &heads,
                    &instance_id,
                    &attempt_id,
                    activation_seq,
                    ActivationFailure {
                        code: "input_contract_violation",
                        safe_message: "node input does not satisfy its selector or schema",
                    },
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        transaction
            .execute_unprepared("SAVEPOINT binding_resolution")
            .await?;
        transaction.execute_raw(sql(
            "INSERT INTO node_instances (id, run_id, node_id, activation_seq, status, graph_revision_id, inputs_object_id, created_at, updated_at) VALUES (?, ?, ?, ?, 'ready', ?, ?, ?, ?)",
            vec![instance_id.clone().into(), run_id.into(), node_id.into(), activation_seq.into(), revision_id.clone().into(), inputs_id.clone().into(), now.into(), now.into()],
        )).await?;
        create_control_snapshot(&transaction, run_id, node, &instance_id, now).await?;
        transaction.execute_raw(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) SELECT ?, ?, 1, 0, 'start', 'queued', control_epoch, 0, ?, execution_manifest_object_id FROM graph_runs WHERE id = ?",
            vec![attempt_id.clone().into(), instance_id.clone().into(), format!("attempt:{instance_id}:1").into(), run_id.into()],
        )).await?;
        let read_result = async {
            resolve_router_reads(&transaction, run_id, &attempt_id, node, now).await?;
            let inputs = load_inputs(&transaction, node, &inputs_id).await?;
            resolve_llm_reads(
                &transaction,
                run_id,
                &instance_id,
                &attempt_id,
                node,
                &inputs,
                now,
            )
            .await
        }
        .await;
        match read_result {
            Ok(()) => {
                transaction
                    .execute_unprepared("RELEASE SAVEPOINT binding_resolution")
                    .await?;
            }
            Err(StorageError::InputContract(_) | StorageError::Domain(_)) => {
                transaction
                    .execute_unprepared("ROLLBACK TO SAVEPOINT binding_resolution")
                    .await?;
                transaction
                    .execute_unprepared("RELEASE SAVEPOINT binding_resolution")
                    .await?;
                fail_input_activation(
                    &transaction,
                    run_id,
                    node,
                    &revision_id,
                    &heads,
                    &instance_id,
                    &attempt_id,
                    activation_seq,
                    ActivationFailure {
                        code: "memory_read_failed",
                        safe_message: "a required node memory binding did not resolve",
                    },
                    now,
                )
                .await?;
                transaction.commit().await?;
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        for head in &heads {
            let consumed = transaction.execute_raw(sql(
                "UPDATE edge_queue_values SET consumed_by_instance_id = ?, consumed_at = ? WHERE id = ? AND consumed_at IS NULL",
                vec![instance_id.clone().into(), now.into(), head.id.clone().into()],
            )).await?;
            if consumed.rows_affected() != 1 {
                return Err(StorageError::Conflict("edge_queue_head"));
            }
            append_event(&transaction, Event {
                run_id,
                event_type: "edge.value.consumed",
                importance: "critical",
                node_instance_id: Some(&instance_id),
                attempt_id: Some(&attempt_id),
                payload: json!({"schemaVersion":1,"queueValueId":head.id,"inputPort":head.port}),
                now,
            }).await?;
            if matches!(&node.kind, DraftNodeKind::Merge { .. }) {
                append_event(
                    &transaction,
                    Event {
                        run_id,
                        event_type: "coordination.merge_selected",
                        importance: "critical",
                        node_instance_id: Some(&instance_id),
                        attempt_id: Some(&attempt_id),
                        payload: json!({
                            "schemaVersion":1,
                            "selectedPort":head.port,
                            "queueValueId":head.id,
                            "enqueueSeq":head.enqueue_seq
                        }),
                        now,
                    },
                )
                .await?;
            }
        }
        if let Some(tuple) = &join_tuple {
            join_by_key::consume(
                &transaction,
                run_id,
                node,
                tuple,
                &instance_id,
                &attempt_id,
                now,
            )
            .await?;
        }
        transaction.execute_raw(sql(
            "UPDATE run_execution_counters SET total_activations = total_activations + 1, total_attempts = total_attempts + 1, pending_queue_values = pending_queue_values - ? WHERE run_id = ?",
            vec![(heads.len() as i64).into(), run_id.into()],
        )).await?;
        add_object_ref(
            &transaction,
            &inputs_id,
            "node_instance",
            &instance_id,
            "inputs",
            now,
        )
        .await?;
        let seq = append_event(
            &transaction,
            Event {
                run_id,
                event_type: "node.scheduled",
                importance: "critical",
                node_instance_id: Some(&instance_id),
                attempt_id: Some(&attempt_id),
                payload: json!({"schemaVersion":1,"nodeId":node_id,"activationSeq":activation_seq}),
                now,
            },
        )
        .await?;
        finish_wakeup(&transaction, wakeup_id).await?;
        enqueue_wakeup(
            &transaction,
            run_id,
            Some(node_id),
            "attempt_ready",
            seq,
            &format!("attempt-ready:{attempt_id}"),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn claimed_wakeup<C: ConnectionTrait>(
    connection: &C,
    id: &str,
    run: &str,
) -> StorageResult<bool> {
    Ok(connection.query_one_raw(sql(
        "SELECT 1 AS present FROM scheduler_wakeups WHERE id = ? AND run_id = ? AND status = 'claimed'",
        vec![id.into(), run.into()],
    )).await?.is_some())
}

pub(super) async fn finish_and_settle<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: &str,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT caused_by_seq FROM scheduler_wakeups WHERE id = ?",
            vec![wakeup_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("scheduler wakeup missing".into()))?;
    let caused_by_seq: i64 = row.try_get("", "caused_by_seq")?;
    finish_wakeup(connection, wakeup_id).await?;
    enqueue_wakeup(
        connection,
        run_id,
        None,
        "settle_run",
        caused_by_seq,
        &format!("settle-after:{wakeup_id}"),
        now,
    )
    .await
}

async fn has_active<C: ConnectionTrait>(
    connection: &C,
    run: &str,
    node: &str,
) -> StorageResult<bool> {
    Ok(connection.query_one_raw(sql(
        "SELECT 1 AS present FROM node_instances WHERE run_id = ? AND node_id = ? AND status IN ('ready','running','waiting')",
        vec![run.into(), node.into()],
    )).await?.is_some())
}

pub(super) async fn allocate_activation_seq<C: ConnectionTrait>(
    connection: &C,
    run: &str,
    node: &str,
) -> StorageResult<i64> {
    let row = connection.query_one_raw(sql(
        "SELECT next_activation_seq FROM node_scheduling_cursors WHERE run_id = ? AND node_id = ?",
        vec![run.into(), node.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("node scheduling cursor missing".into()))?;
    let seq: i64 = row.try_get("", "next_activation_seq")?;
    let updated = connection.execute_raw(sql(
        "UPDATE node_scheduling_cursors SET next_activation_seq = next_activation_seq + 1 WHERE run_id = ? AND node_id = ? AND next_activation_seq = ?",
        vec![run.into(), node.into(), seq.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("node_activation_sequence"));
    }
    Ok(seq)
}

pub(super) async fn limits_exceeded<C: ConnectionTrait>(
    connection: &C,
    run: &str,
    limits: &zhuangsheng_core::graph::RunLimits,
) -> StorageResult<bool> {
    let row = connection
        .query_one_raw(sql(
            "SELECT total_activations, total_attempts FROM run_execution_counters WHERE run_id = ?",
            vec![run.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let activations: i64 = row.try_get("", "total_activations")?;
    let attempts: i64 = row.try_get("", "total_attempts")?;
    Ok(
        activations.saturating_add(1) as u64 > limits.max_node_activations
            || attempts.saturating_add(1) as u64
                > limits
                    .max_node_activations
                    .saturating_mul(limits.max_attempts_per_activation),
    )
}
