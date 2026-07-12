use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{AppliedGraphDefinition, DraftNodeKind, GraphNode},
    schema, selector,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

use super::{
    activate::{allocate_activation_seq, finish_and_settle, limits_exceeded},
    activation_failure::{ActivationFailure, fail_input_activation},
    activation_inputs::{QueueHead, build_inputs, queue_heads},
    aggregator_close::close_window,
    events::{Event, add_object_ref, append_event, fail_run},
};

pub(super) async fn activate<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: &str,
    run_id: &str,
    node: &GraphNode,
    revision_id: &str,
    definition: &AppliedGraphDefinition,
    now: i64,
) -> StorageResult<()> {
    let open = connection.query_one_raw(sql(
        "SELECT id, node_instance_id, open_attempt_id, item_count FROM aggregation_windows WHERE run_id = ? AND node_id = ? AND status = 'open'",
        vec![run_id.into(), node.id.clone().into()],
    )).await?;
    let Some(heads) = queue_heads(connection, run_id, node, &definition.edges).await? else {
        finish_and_settle(connection, wakeup_id, run_id, now).await?;
        return Ok(());
    };
    let head = heads
        .into_iter()
        .next()
        .ok_or_else(|| StorageError::Integrity("aggregator queue head missing".into()))?;
    if let Some(open) = open {
        let window_id: String = open.try_get("", "id")?;
        let instance_id: String = open.try_get("", "node_instance_id")?;
        let open_attempt_id: String = open.try_get("", "open_attempt_id")?;
        let item_count: i64 = open.try_get("", "item_count")?;
        return advance(
            connection,
            wakeup_id,
            run_id,
            node,
            definition,
            &window_id,
            &instance_id,
            &open_attempt_id,
            item_count,
            head,
            now,
        )
        .await;
    }
    open_window(
        connection,
        wakeup_id,
        run_id,
        node,
        revision_id,
        definition,
        head,
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn open_window<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: &str,
    run_id: &str,
    node: &GraphNode,
    revision_id: &str,
    definition: &AppliedGraphDefinition,
    head: QueueHead,
    now: i64,
) -> StorageResult<()> {
    if limits_exceeded(connection, run_id, &definition.limits).await? {
        return fail_run(
            connection,
            run_id,
            "run_limit_exceeded",
            "activation limit exceeded",
            now,
        )
        .await;
    }
    if !has_buffer_capacity(connection, run_id, &definition.limits).await? {
        return fail_run(
            connection,
            run_id,
            "run_limit_exceeded",
            "coordinator buffered value limit exceeded",
            now,
        )
        .await;
    }
    let instance_id = new_id("nodeinst");
    let attempt_id = new_id("attempt");
    let activation_seq = allocate_activation_seq(connection, run_id, &node.id).await?;
    let inputs_id = match build_inputs(
        connection,
        node,
        std::slice::from_ref(&head),
        &instance_id,
        None,
        now,
    )
    .await
    {
        Ok(id) => id,
        Err(StorageError::InputContract(_) | StorageError::Domain(_)) => {
            fail_input_activation(
                connection,
                run_id,
                node,
                revision_id,
                std::slice::from_ref(&head),
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
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let selected_id = selected_value_id(connection, node, &head, now).await?;
    let DraftNodeKind::Aggregator { count } = &node.kind else {
        unreachable!()
    };
    let timeout_ms = node
        .timeout_ms
        .ok_or_else(|| StorageError::Integrity("aggregator timeout missing".into()))?;
    let deadline = now.saturating_add(i64::try_from(timeout_ms).unwrap_or(i64::MAX));
    let window_id = new_id("aggwin");
    connection.execute_raw(sql(
        "INSERT INTO node_instances (id, run_id, node_id, activation_seq, status, graph_revision_id, inputs_object_id, created_at, updated_at) VALUES (?, ?, ?, ?, 'waiting', ?, ?, ?, ?)",
        vec![instance_id.clone().into(), run_id.into(), node.id.clone().into(), activation_seq.into(), revision_id.into(), inputs_id.clone().into(), now.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, result_idempotency_key, executor_object_id, finished_at) SELECT ?, ?, 1, 0, 'start', 'completed', control_epoch, 0, ?, ?, execution_manifest_object_id, ? FROM graph_runs WHERE id = ?",
        vec![attempt_id.clone().into(), instance_id.clone().into(), format!("attempt:{instance_id}:1").into(), format!("aggregation-open:{window_id}").into(), now.into(), run_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO aggregation_windows (id, run_id, node_id, node_instance_id, open_attempt_id, status, count_limit, item_count, opened_at, deadline_at) VALUES (?, ?, ?, ?, ?, 'open', ?, 1, ?, ?)",
        vec![window_id.clone().into(), run_id.into(), node.id.clone().into(), instance_id.clone().into(), attempt_id.clone().into(), (*count as i64).into(), now.into(), deadline.into()],
    )).await?;
    append_item(
        connection,
        run_id,
        &window_id,
        &instance_id,
        &attempt_id,
        0,
        &head,
        &selected_id,
        now,
    )
    .await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET total_activations = total_activations + 1, total_attempts = total_attempts + 1, pending_queue_values = pending_queue_values - 1, coordinator_buffered_values = coordinator_buffered_values + 1 WHERE run_id = ?",
        vec![run_id.into()],
    )).await?;
    add_object_ref(
        connection,
        &inputs_id,
        "node_instance",
        &instance_id,
        "inputs",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id,
            event_type: "node.scheduled",
            importance: "critical",
            node_instance_id: Some(&instance_id),
            attempt_id: Some(&attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":node.id,"activationSeq":activation_seq}),
            now,
        },
    )
    .await?;
    append_event(connection, Event { run_id, event_type: "coordination.window_opened", importance: "critical", node_instance_id: Some(&instance_id), attempt_id: Some(&attempt_id), payload: json!({"schemaVersion":1,"windowId":window_id,"deadlineAt":deadline,"count":count}), now }).await?;
    if *count == 1 {
        close_window(
            connection,
            Some(wakeup_id),
            run_id,
            node,
            definition,
            &window_id,
            &instance_id,
            &attempt_id,
            "count",
            false,
            now,
        )
        .await
    } else {
        finish_and_settle(connection, wakeup_id, run_id, now).await
    }
}

#[allow(clippy::too_many_arguments)]
async fn advance<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: &str,
    run_id: &str,
    node: &GraphNode,
    definition: &AppliedGraphDefinition,
    window_id: &str,
    instance_id: &str,
    open_attempt_id: &str,
    item_count: i64,
    head: QueueHead,
    now: i64,
) -> StorageResult<()> {
    if !has_buffer_capacity(connection, run_id, &definition.limits).await? {
        fail_run(
            connection,
            run_id,
            "run_limit_exceeded",
            "coordinator buffered value limit exceeded",
            now,
        )
        .await?;
        return Ok(());
    }
    let selected_id = match selected_value_id(connection, node, &head, now).await {
        Ok(id) => id,
        Err(StorageError::InputContract(_) | StorageError::Domain(_)) => {
            fail_run(
                connection,
                run_id,
                "input_contract_violation",
                "aggregator input does not satisfy its selector or schema",
                now,
            )
            .await?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let next_count = item_count + 1;
    append_item(
        connection,
        run_id,
        window_id,
        instance_id,
        open_attempt_id,
        item_count,
        &head,
        &selected_id,
        now,
    )
    .await?;
    connection.execute_raw(sql(
        "UPDATE aggregation_windows SET item_count = ? WHERE id = ? AND status = 'open' AND item_count = ?",
        vec![next_count.into(), window_id.into(), item_count.into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET pending_queue_values = pending_queue_values - 1, coordinator_buffered_values = coordinator_buffered_values + 1 WHERE run_id = ?",
        vec![run_id.into()],
    )).await?;
    let DraftNodeKind::Aggregator { count, .. } = &node.kind else {
        unreachable!()
    };
    if next_count as u64 >= *count {
        close_window(
            connection,
            Some(wakeup_id),
            run_id,
            node,
            definition,
            window_id,
            instance_id,
            open_attempt_id,
            "count",
            true,
            now,
        )
        .await
    } else {
        finish_and_settle(connection, wakeup_id, run_id, now).await
    }
}

async fn has_buffer_capacity<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    limits: &zhuangsheng_core::graph::RunLimits,
) -> StorageResult<bool> {
    let row = connection
        .query_one_raw(sql(
            "SELECT coordinator_buffered_values FROM run_execution_counters WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let current: i64 = row.try_get("", "coordinator_buffered_values")?;
    Ok(current.saturating_add(1) as u64 <= limits.max_coordinator_buffered_values)
}

#[allow(clippy::too_many_arguments)]
async fn append_item<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    window_id: &str,
    instance_id: &str,
    attempt_id: &str,
    index: i64,
    head: &QueueHead,
    selected_id: &str,
    now: i64,
) -> StorageResult<()> {
    let consumed = connection.execute_raw(sql(
        "UPDATE edge_queue_values SET consumed_by_instance_id = ?, consumed_at = ? WHERE id = ? AND consumed_at IS NULL",
        vec![instance_id.into(), now.into(), head.id.clone().into()],
    )).await?;
    if consumed.rows_affected() != 1 {
        return Err(StorageError::Conflict("aggregator_queue_head"));
    }
    connection.execute_raw(sql(
        "INSERT INTO aggregation_window_items (window_id, item_index, queue_value_id, enqueue_seq, selected_value_object_id, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        vec![window_id.into(), index.into(), head.id.clone().into(), head.enqueue_seq.into(), selected_id.into(), now.into()],
    )).await?;
    add_object_ref(
        connection,
        selected_id,
        "aggregation_window",
        window_id,
        &format!("item:{index}"),
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id,
            event_type: "edge.value.consumed",
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: Some(attempt_id),
            payload: json!({"schemaVersion":1,"queueValueId":head.id,"inputPort":head.port}),
            now,
        },
    )
    .await?;
    append_event(connection, Event { run_id, event_type: "coordination.window_item_added", importance: "critical", node_instance_id: Some(instance_id), attempt_id: Some(attempt_id), payload: json!({"schemaVersion":1,"windowId":window_id,"itemIndex":index,"queueValueId":head.id,"enqueueSeq":head.enqueue_seq}), now }).await?;
    Ok(())
}

async fn selected_value_id<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    head: &QueueHead,
    now: i64,
) -> StorageResult<String> {
    let input = node
        .inputs
        .first()
        .ok_or_else(|| StorageError::Integrity("aggregator input missing".into()))?;
    let raw: Value = load_object_json(connection, &head.value_object_id).await?;
    let selected = selector::select(&input.binding.selector, &raw, 100_000)
        .map_err(StorageError::InputContract)?;
    if let Some(spec) = &input.schema {
        schema::validate(spec, &selected)?;
    }
    put_inline_object(connection, &canonical::to_vec(&selected)?, now).await
}
