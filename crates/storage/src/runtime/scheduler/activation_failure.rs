use sea_orm::ConnectionTrait;
use serde::Serialize;
use serde_json::{Map, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::{
    activate::QueueHead,
    events::{Event, add_object_ref, append_event, fail_run},
    load::object_metadata,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RawValueRef<'a> {
    id: &'a str,
    content_hash: &'a str,
    encoding: &'static str,
    size_bytes: i64,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn fail_input_activation<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    revision_id: &str,
    heads: &[QueueHead],
    instance_id: &str,
    attempt_id: &str,
    activation_seq: i64,
    now: i64,
) -> StorageResult<()> {
    let inputs_id = failed_inputs(connection, heads, instance_id, now).await?;
    let safe_message = "node input does not satisfy its selector or schema";
    let error_id = put_inline_object(
        connection,
        &canonical::to_vec(&json!({
            "schemaVersion":1,
            "code":"input_contract_violation",
            "safeMessage":safe_message,
            "retryClass":"never"
        }))?,
        now,
    )
    .await?;
    connection.execute_raw(sql(
        "INSERT INTO node_instances (id, run_id, node_id, activation_seq, status, graph_revision_id, inputs_object_id, created_at, updated_at) VALUES (?, ?, ?, ?, 'failed', ?, ?, ?, ?)",
        vec![instance_id.into(), run_id.into(), node.id.clone().into(), activation_seq.into(), revision_id.into(), inputs_id.clone().into(), now.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, result_idempotency_key, executor_object_id, error_object_id, finished_at) SELECT ?, ?, 1, 0, 'start', 'failed', control_epoch, 0, ?, ?, execution_manifest_object_id, ?, ? FROM graph_runs WHERE id = ?",
        vec![attempt_id.into(), instance_id.into(), format!("attempt:{instance_id}:1").into(), format!("activation-failure:{instance_id}").into(), error_id.clone().into(), now.into(), run_id.into()],
    )).await?;
    consume_heads(
        connection,
        run_id,
        node,
        heads,
        instance_id,
        attempt_id,
        now,
    )
    .await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET total_activations = total_activations + 1, total_attempts = total_attempts + 1, pending_queue_values = pending_queue_values - ? WHERE run_id = ?",
        vec![(heads.len() as i64).into(), run_id.into()],
    )).await?;
    add_object_ref(
        connection,
        &inputs_id,
        "node_instance",
        instance_id,
        "inputs",
        now,
    )
    .await?;
    add_object_ref(
        connection,
        &error_id,
        "node_attempt",
        attempt_id,
        "error",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id,
            event_type: "node.scheduled",
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: Some(attempt_id),
            payload: json!({"schemaVersion":1,"nodeId":node.id,"activationSeq":activation_seq}),
            now,
        },
    )
    .await?;
    append_event(connection, Event {
        run_id,
        event_type: "node.failed",
        importance: "critical",
        node_instance_id: Some(instance_id),
        attempt_id: Some(attempt_id),
        payload: json!({"schemaVersion":1,"nodeId":node.id,"code":"input_contract_violation","safeMessage":safe_message}),
        now,
    }).await?;
    fail_run(
        connection,
        run_id,
        "input_contract_violation",
        safe_message,
        now,
    )
    .await
}

async fn failed_inputs<C: ConnectionTrait>(
    connection: &C,
    heads: &[QueueHead],
    instance_id: &str,
    now: i64,
) -> StorageResult<String> {
    let mut ports = Map::new();
    for head in heads {
        let (content_hash, size_bytes) = object_metadata(connection, &head.value_object_id).await?;
        ports.insert(head.port.clone(), json!({
            "rawValue":RawValueRef { id:&head.value_object_id, content_hash:&content_hash, encoding:"canonical_json_v1", size_bytes },
            "queueItemIds":[head.id]
        }));
        add_object_ref(
            connection,
            &head.value_object_id,
            "node_instance",
            instance_id,
            &format!("raw_input:{}", head.port),
            now,
        )
        .await?;
    }
    put_inline_object(
        connection,
        &canonical::to_vec(&json!({
            "schemaVersion":1,
            "ports":ports,
            "activationError":{"code":"input_contract_violation"}
        }))?,
        now,
    )
    .await
}

async fn consume_heads<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    heads: &[QueueHead],
    instance_id: &str,
    attempt_id: &str,
    now: i64,
) -> StorageResult<()> {
    for head in heads {
        let consumed = connection.execute_raw(sql(
            "UPDATE edge_queue_values SET consumed_by_instance_id = ?, consumed_at = ? WHERE id = ? AND consumed_at IS NULL",
            vec![instance_id.into(), now.into(), head.id.clone().into()],
        )).await?;
        if consumed.rows_affected() != 1 {
            return Err(StorageError::Conflict("edge_queue_head"));
        }
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
        if matches!(&node.kind, DraftNodeKind::Merge { .. }) {
            append_event(connection, Event {
                run_id,
                event_type: "coordination.merge_selected",
                importance: "critical",
                node_instance_id: Some(instance_id),
                attempt_id: Some(attempt_id),
                payload: json!({"schemaVersion":1,"selectedPort":head.port,"queueValueId":head.id,"enqueueSeq":head.enqueue_seq}),
                now,
            }).await?;
        }
    }
    Ok(())
}
