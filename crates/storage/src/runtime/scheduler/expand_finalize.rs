use std::collections::HashSet;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{AppliedGraphDefinition, DraftNodeKind, GraphNode},
    scheduler::FinalizeAttemptCommand,
    schema,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{
    attempt_finish::{complete_rows, fail_attempt, settle_interrupt_after_attempt},
    attempt_state::AttemptState,
    emit::StoredValue,
    events::{Event, add_object_ref, append_event, enqueue_wakeup, finish_wakeup},
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn finalize<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    node: &GraphNode,
    definition: &AppliedGraphDefinition,
    output: &str,
    values: &[Value],
    now: i64,
) -> StorageResult<()> {
    let DraftNodeKind::Expand { max_items } = &node.kind else {
        return Err(StorageError::Integrity(
            "expanded result belongs to a non-expand node".into(),
        ));
    };
    if values.len() as u64 > *max_items
        || node.outputs.first().map(|port| port.name.as_str()) != Some(output)
    {
        fail_attempt(
            connection,
            state,
            command,
            "node_output_contract_violation",
            "Expand output does not match its applied contract",
            now,
        )
        .await?;
        return Ok(());
    }
    let stored = match prepare_values(connection, node, values, definition, now).await {
        Ok(stored) => stored,
        Err(StorageError::InputContract(_) | StorageError::Domain(_)) => {
            fail_attempt(
                connection,
                state,
                command,
                "node_output_contract_violation",
                "Expand element violates its output contract",
                now,
            )
            .await?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let edges: Vec<_> = {
        let mut edges: Vec<_> = definition
            .edges
            .iter()
            .filter(|edge| edge.from.node_id == node.id && edge.from.output == output)
            .collect();
        edges.sort_by(|left, right| left.id.cmp(&right.id));
        edges
    };
    let queue_count = values
        .len()
        .checked_mul(edges.len())
        .and_then(|count| i64::try_from(count).ok())
        .ok_or_else(|| StorageError::InputContract("Expand queue count overflow".into()))?;
    let first_seq =
        match reserve_queue_capacity(connection, &state.run_id, queue_count, definition).await {
            Ok(sequence) => sequence,
            Err(StorageError::InputContract(_)) => {
                fail_attempt(
                    connection,
                    state,
                    command,
                    "run_limit_exceeded",
                    "Expand batch exceeds run queue limits",
                    now,
                )
                .await?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
    let queue_ids = emit_values(
        connection,
        state,
        &stored,
        &edges,
        &command.attempt_id,
        first_seq,
        now,
    )
    .await?;
    let final_outputs = put_inline_object(
        connection,
        &canonical::to_vec(&json!({
            "schemaVersion":1,
            "expandedOutput":output,
            "values":stored
        }))?,
        now,
    )
    .await?;
    complete_rows(connection, state, command, &final_outputs, now).await?;
    add_object_ref(
        connection,
        &final_outputs,
        "node_instance",
        &state.node_instance_id,
        "final_outputs",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id: &state.run_id,
            event_type: "coordination.expand_completed",
            importance: "critical",
            node_instance_id: Some(&state.node_instance_id),
            attempt_id: Some(&command.attempt_id),
            payload: json!({
                "schemaVersion":1,
                "outputPort":output,
                "itemCount":stored.len(),
                "queueValueIds":queue_ids
            }),
            now,
        },
    )
    .await?;
    let seq = append_event(
        connection,
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
    finish_wakeup(connection, &command.wakeup_id).await?;
    enqueue_wakeup(
        connection,
        &state.run_id,
        Some(&state.node_id),
        "node_maybe_ready",
        seq,
        &format!("recheck:{}", state.node_instance_id),
        now,
    )
    .await?;
    enqueue_wakeup(
        connection,
        &state.run_id,
        None,
        "settle_run",
        seq,
        &format!("settle:{}", state.node_instance_id),
        now,
    )
    .await?;
    settle_interrupt_after_attempt(connection, &state.run_id, now).await
}

async fn prepare_values<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    values: &[Value],
    definition: &AppliedGraphDefinition,
    now: i64,
) -> StorageResult<Vec<StoredValue>> {
    let port = node
        .outputs
        .first()
        .ok_or_else(|| StorageError::Integrity("Expand output port missing".into()))?;
    let mut stored = Vec::with_capacity(values.len());
    for value in values {
        if let Some(spec) = &port.schema {
            schema::validate(spec, value)?;
        }
        let bytes = canonical::to_vec(value)?;
        if bytes.len() as u64 > definition.limits.max_value_bytes {
            return Err(StorageError::InputContract(
                "Expand element exceeds value limit".into(),
            ));
        }
        stored.push(StoredValue {
            id: put_inline_object(connection, &bytes, now).await?,
            content_hash: canonical::hash_bytes(&bytes),
            encoding: "canonical_json_v1",
            size_bytes: bytes.len() as i64,
        });
    }
    Ok(stored)
}

async fn reserve_queue_capacity<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    count: i64,
    definition: &AppliedGraphDefinition,
) -> StorageResult<i64> {
    let row = connection.query_one_raw(sql(
        "SELECT next_enqueue_seq, total_queue_values, pending_queue_values FROM run_execution_counters WHERE run_id = ?",
        vec![run_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let first: i64 = row.try_get("", "next_enqueue_seq")?;
    let total: i64 = row.try_get("", "total_queue_values")?;
    let pending: i64 = row.try_get("", "pending_queue_values")?;
    if total.saturating_add(count) as u64 > definition.limits.max_total_queue_values
        || pending.saturating_add(count) as u64 > definition.limits.max_pending_queue_values
    {
        return Err(StorageError::InputContract(
            "Expand batch exceeds queue limits".into(),
        ));
    }
    if count > 0 {
        let updated = connection.execute_raw(sql(
            "UPDATE run_execution_counters SET next_enqueue_seq = next_enqueue_seq + ?, total_queue_values = total_queue_values + ?, pending_queue_values = pending_queue_values + ? WHERE run_id = ? AND next_enqueue_seq = ?",
            vec![count.into(), count.into(), count.into(), run_id.into(), first.into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("run_enqueue_sequence"));
        }
    }
    Ok(first)
}

async fn emit_values<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    values: &[StoredValue],
    edges: &[&zhuangsheng_core::graph::GraphEdge],
    attempt_id: &str,
    first_seq: i64,
    now: i64,
) -> StorageResult<Vec<String>> {
    let mut queue_ids = Vec::with_capacity(values.len().saturating_mul(edges.len()));
    let mut downstream = HashSet::new();
    for (item_index, value) in values.iter().enumerate() {
        for (edge_index, edge) in edges.iter().enumerate() {
            let queue_id = new_id("queue");
            let offset = item_index
                .checked_mul(edges.len())
                .and_then(|value| value.checked_add(edge_index))
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| StorageError::Integrity("Expand sequence overflow".into()))?;
            let enqueue_seq = first_seq.saturating_add(offset);
            connection.execute_raw(sql(
                "INSERT INTO edge_queue_values (id, run_id, edge_id, enqueue_seq, producer_instance_id, producer_emission_index, value_object_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                vec![queue_id.clone().into(), state.run_id.clone().into(), edge.id.clone().into(), enqueue_seq.into(), state.node_instance_id.clone().into(), (item_index as i64).into(), value.id.clone().into(), now.into()],
            )).await?;
            add_object_ref(
                connection,
                &value.id,
                "edge_queue_value",
                &queue_id,
                "value",
                now,
            )
            .await?;
            let event_seq = append_event(connection, Event {
                run_id: &state.run_id,
                event_type: "edge.value.enqueued",
                importance: "critical",
                node_instance_id: Some(&state.node_instance_id),
                attempt_id: Some(attempt_id),
                payload: json!({"schemaVersion":1,"queueValueId":queue_id,"edgeId":edge.id,"enqueueSeq":enqueue_seq,"emissionIndex":item_index}),
                now,
            }).await?;
            if downstream.insert(edge.to.node_id.clone()) {
                enqueue_wakeup(
                    connection,
                    &state.run_id,
                    Some(&edge.to.node_id),
                    "node_maybe_ready",
                    event_seq,
                    &format!(
                        "expand-edge-ready:{}:{}",
                        state.node_instance_id, edge.to.node_id
                    ),
                    now,
                )
                .await?;
            }
            queue_ids.push(queue_id);
        }
    }
    Ok(queue_ids)
}
