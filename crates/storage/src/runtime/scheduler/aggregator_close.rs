use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{AppliedGraphDefinition, GraphNode},
};

use crate::{
    StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{load_object_json, new_id, put_inline_object, sql},
    },
};

use super::{
    emit::{emit_edges, prepare_outputs},
    events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run, finish_wakeup},
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn close_window<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: Option<&str>,
    run_id: &str,
    node: &GraphNode,
    definition: &AppliedGraphDefinition,
    window_id: &str,
    instance_id: &str,
    open_attempt_id: &str,
    reason: &str,
    create_resume: bool,
    now: i64,
) -> StorageResult<()> {
    if create_resume && !has_resume_capacity(connection, run_id, definition).await? {
        fail_run(
            connection,
            run_id,
            "run_limit_exceeded",
            "aggregator resume attempt limit exceeded",
            now,
        )
        .await?;
        return Ok(());
    }
    let rows = connection.query_all_raw(sql(
        "SELECT selected_value_object_id FROM aggregation_window_items WHERE window_id = ? ORDER BY item_index",
        vec![window_id.into()],
    )).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(
            load_object_json::<_, Value>(
                connection,
                &row.try_get::<String>("", "selected_value_object_id")?,
            )
            .await?,
        );
    }
    let output = node
        .outputs
        .first()
        .ok_or_else(|| StorageError::Integrity("aggregator output missing".into()))?;
    let outputs = BTreeMap::from([(
        output.name.clone(),
        json!({"items":items,"closeReason":reason}),
    )]);
    let stored =
        match prepare_outputs(connection, node, &outputs, None, &definition.limits, now).await {
            Ok(value) => value,
            Err(StorageError::InputContract(_) | StorageError::Domain(_)) => {
                fail_run(
                    connection,
                    run_id,
                    "node_output_contract_violation",
                    "aggregator output violates its contract",
                    now,
                )
                .await?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
    if let Err(error) = emit_edges(
        connection,
        run_id,
        instance_id,
        node,
        definition,
        &stored,
        None,
        &definition.limits,
        now,
    )
    .await
    {
        if matches!(error, StorageError::InputContract(_)) {
            fail_run(
                connection,
                run_id,
                "run_limit_exceeded",
                "aggregator output exceeds queue limits",
                now,
            )
            .await?;
            return Ok(());
        }
        return Err(error);
    }
    let final_outputs = put_inline_object(
        connection,
        &canonical::to_vec(&json!({"schemaVersion":1,"outputs":stored}))?,
        now,
    )
    .await?;
    let close_attempt_id = if create_resume {
        let id = new_id("attempt");
        connection.execute_raw(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, result_idempotency_key, executor_object_id, finished_at) SELECT ?, ?, 2, 0, 'resume', 'completed', control_epoch, 0, ?, ?, execution_manifest_object_id, ? FROM graph_runs WHERE id = ?",
            vec![id.clone().into(), instance_id.into(), format!("aggregation-close:{window_id}").into(), format!("aggregation-result:{window_id}").into(), now.into(), run_id.into()],
        )).await?;
        connection.execute_raw(sql("UPDATE run_execution_counters SET total_attempts = total_attempts + 1 WHERE run_id = ?", vec![run_id.into()])).await?;
        id
    } else {
        open_attempt_id.into()
    };
    connection.execute_raw(sql(
        "UPDATE aggregation_windows SET status = 'completed', close_reason = ?, closed_at = ? WHERE id = ? AND status = 'open'",
        vec![reason.into(), now.into(), window_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE node_instances SET status = 'completed', final_outputs_object_id = ?, updated_at = ? WHERE id = ? AND status = 'waiting'",
        vec![final_outputs.clone().into(), now.into(), instance_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET coordinator_buffered_values = coordinator_buffered_values - ? WHERE run_id = ?",
        vec![(rows.len() as i64).into(), run_id.into()],
    )).await?;
    add_object_ref(
        connection,
        &final_outputs,
        "node_instance",
        instance_id,
        "final_outputs",
        now,
    )
    .await?;
    append_event(connection, Event { run_id, event_type: "coordination.window_closed", importance: "critical", node_instance_id: Some(instance_id), attempt_id: Some(&close_attempt_id), payload: json!({"schemaVersion":1,"windowId":window_id,"closeReason":reason,"itemCount":rows.len()}), now }).await?;
    let seq = append_event(
        connection,
        Event {
            run_id,
            event_type: "node.completed",
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: Some(&close_attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":node.id}),
            now,
        },
    )
    .await?;
    if let Some(wakeup_id) = wakeup_id {
        finish_wakeup(connection, wakeup_id).await?;
    }
    enqueue_wakeup(
        connection,
        run_id,
        Some(&node.id),
        "node_maybe_ready",
        seq,
        &format!("aggregation-recheck:{window_id}"),
        now,
    )
    .await?;
    enqueue_wakeup(
        connection,
        run_id,
        None,
        "settle_run",
        seq,
        &format!("aggregation-settle:{window_id}"),
        now,
    )
    .await?;
    Ok(())
}

async fn has_resume_capacity<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    definition: &AppliedGraphDefinition,
) -> StorageResult<bool> {
    let row = connection
        .query_one_raw(sql(
            "SELECT total_attempts FROM run_execution_counters WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let attempts: i64 = row.try_get("", "total_attempts")?;
    let maximum = definition
        .limits
        .max_node_activations
        .saturating_mul(definition.limits.max_attempts_per_activation);
    Ok(attempts.saturating_add(1) as u64 <= maximum)
}

pub(super) async fn close_due<C: ConnectionTrait>(
    connection: &C,
    window_id: &str,
    now: i64,
) -> StorageResult<()> {
    let row = connection.query_one_raw(sql(
        "SELECT w.run_id, w.node_id, w.node_instance_id, w.open_attempt_id, ni.graph_revision_id FROM aggregation_windows w JOIN node_instances ni ON ni.id = w.node_instance_id JOIN graph_runs r ON r.id = w.run_id WHERE w.id = ? AND w.status = 'open' AND w.deadline_at <= ? AND r.status IN ('running','waiting')",
        vec![window_id.into(), now.into()],
    )).await?;
    let Some(row) = row else { return Ok(()) };
    let run_id: String = row.try_get("", "run_id")?;
    let revision =
        load_revision(connection, &row.try_get::<String>("", "graph_revision_id")?).await?;
    let node_id: String = row.try_get("", "node_id")?;
    let node = revision
        .definition
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| StorageError::Integrity("aggregator node missing".into()))?;
    connection.execute_raw(sql("UPDATE graph_runs SET status = 'running', updated_at = ? WHERE id = ? AND status = 'waiting'", vec![now.into(), run_id.clone().into()])).await?;
    close_window(
        connection,
        None,
        &run_id,
        node,
        &revision.definition,
        window_id,
        &row.try_get::<String>("", "node_instance_id")?,
        &row.try_get::<String>("", "open_attempt_id")?,
        "timeout",
        true,
        now,
    )
    .await
}
