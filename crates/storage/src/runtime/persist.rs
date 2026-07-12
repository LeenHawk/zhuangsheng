use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{canonical, graph::GraphNode, runtime::StartRunCommand};

use crate::{StorageResult, graph::helpers::*};

use super::{context::ContextBinding, start::PreparedInput};

pub(super) struct RuntimeRows<'a> {
    pub run_id: &'a str,
    pub graph_revision_id: &'a str,
    pub nodes: &'a [GraphNode],
    pub prepared: &'a [PreparedInput],
    pub manifest_id: &'a str,
    pub binding: &'a ContextBinding,
    pub now: i64,
}

struct EventInsert<'a> {
    run_id: &'a str,
    seq: i64,
    event_type: &'a str,
    node_instance_id: Option<&'a str>,
    attempt_id: Option<&'a str>,
    payload: serde_json::Value,
    now: i64,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_run<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    scope: &str,
    command: &StartRunCommand,
    digest: &str,
    graph_content_hash: &str,
    manifest_id: &str,
    binding: &ContextBinding,
    limits_id: &str,
    input_id: &str,
    deadline_at: i64,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO graph_runs (id, request_idempotency_scope, request_idempotency_key, request_digest, graph_revision_id, graph_content_hash, execution_manifest_object_id, context_id, branch_id, input_commit_id, status, control_epoch, limits_object_id, run_input_object_id, started_at, deadline_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'running', 0, ?, ?, ?, ?, ?, ?)",
        vec![run_id.into(), scope.into(), command.idempotency_key.clone().into(), digest.into(), command.graph_revision_id.clone().into(), graph_content_hash.into(), manifest_id.into(), binding.context_id.clone().into(), binding.branch_id.clone().into(), binding.input_commit_id.clone().into(), limits_id.into(), input_id.into(), now.into(), deadline_at.into(), now.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO runtime_timers (id, run_id, kind, due_at, dedupe_key, status, created_at) VALUES (?, ?, 'run_deadline', ?, ?, 'pending', ?)",
        vec![new_id("timer").into(), run_id.into(), deadline_at.into(), format!("run-deadline:{run_id}").into(), now.into()],
    )).await?;
    Ok(())
}

pub(super) async fn insert_runtime_rows<C: ConnectionTrait>(
    connection: &C,
    rows: RuntimeRows<'_>,
) -> StorageResult<()> {
    let RuntimeRows {
        run_id,
        graph_revision_id,
        nodes,
        prepared,
        manifest_id,
        binding,
        now,
    } = rows;
    let input_count = prepared.len() as i64;
    connection.execute_raw(sql(
        "INSERT INTO run_execution_counters (run_id, next_enqueue_seq, next_output_seq, total_activations, total_attempts, total_queue_values, pending_queue_values, open_waits, coordinator_buffered_values) VALUES (?, 1, 1, ?, ?, 0, 0, 0, 0)",
        vec![run_id.into(), input_count.into(), input_count.into()],
    )).await?;
    for node in nodes {
        let next = if node.is_entry { 2_i64 } else { 1_i64 };
        connection.execute_raw(sql(
            "INSERT INTO node_scheduling_cursors (run_id, node_id, next_activation_seq) VALUES (?, ?, ?)",
            vec![run_id.into(), node.id.clone().into(), next.into()],
        )).await?;
    }
    let next_event_seq = prepared.len() as i64 + 3;
    connection
        .execute_raw(sql(
            "INSERT INTO run_event_counters (run_id, next_seq) VALUES (?, ?)",
            vec![run_id.into(), next_event_seq.into()],
        ))
        .await?;
    insert_event(
        connection,
        EventInsert {
            run_id,
            seq: 1,
            event_type: "run.created",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({
                "schemaVersion": 1,
                "graphRevisionId": graph_revision_id,
                "contextId": binding.context_id,
                "branchId": binding.branch_id,
                "inputCommitId": binding.input_commit_id,
            }),
            now,
        },
    )
    .await?;
    insert_event(
        connection,
        EventInsert {
            run_id,
            seq: 2,
            event_type: "run.started",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1}),
            now,
        },
    )
    .await?;

    for (index, item) in prepared.iter().enumerate() {
        connection.execute_raw(sql(
            "INSERT INTO node_instances (id, run_id, node_id, activation_seq, status, graph_revision_id, inputs_object_id, created_at, updated_at) VALUES (?, ?, ?, 1, 'ready', ?, ?, ?, ?)",
            vec![item.instance_id.clone().into(), run_id.into(), item.node_id.clone().into(), graph_revision_id.into(), item.inputs_object_id.clone().into(), now.into(), now.into()],
        )).await?;
        connection.execute_raw(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) VALUES (?, ?, 1, 0, 'start', 'queued', 0, 0, ?, ?)",
            vec![item.attempt_id.clone().into(), item.instance_id.clone().into(), format!("attempt:{}:1", item.instance_id).into(), manifest_id.into()],
        )).await?;
        add_ref(
            connection,
            &item.inputs_object_id,
            "node_instance",
            &item.instance_id,
            "inputs",
            now,
        )
        .await?;
        add_ref(
            connection,
            &item.selected_object_id,
            "node_instance",
            &item.instance_id,
            "selected_source",
            now,
        )
        .await?;
        add_ref(
            connection,
            manifest_id,
            "node_attempt",
            &item.attempt_id,
            "executor",
            now,
        )
        .await?;
        let seq = index as i64 + 3;
        insert_event(
            connection,
            EventInsert {
                run_id,
                seq,
                event_type: "node.scheduled",
                node_instance_id: Some(&item.instance_id),
                attempt_id: Some(&item.attempt_id),
                payload: json!({"schemaVersion":1,"nodeId":item.node_id,"activationSeq":1}),
                now,
            },
        )
        .await?;
        connection.execute_raw(sql(
            "INSERT INTO scheduler_wakeups (id, run_id, node_id, kind, caused_by_seq, dedupe_key, status, available_at, created_at) VALUES (?, ?, ?, 'attempt_ready', ?, ?, 'pending', ?, ?)",
            vec![new_id("wakeup").into(), run_id.into(), item.node_id.clone().into(), seq.into(), format!("attempt-ready:{}", item.attempt_id).into(), now.into(), now.into()],
        )).await?;
    }
    Ok(())
}

pub(super) async fn add_run_refs<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    input_id: &str,
    manifest_id: &str,
    limits_id: &str,
    now: i64,
) -> StorageResult<()> {
    add_ref(connection, input_id, "graph_run", run_id, "run_input", now).await?;
    add_ref(
        connection,
        manifest_id,
        "graph_run",
        run_id,
        "execution_manifest",
        now,
    )
    .await?;
    add_ref(connection, limits_id, "graph_run", run_id, "limits", now).await?;
    Ok(())
}

async fn insert_event<C: ConnectionTrait>(
    connection: &C,
    event: EventInsert<'_>,
) -> StorageResult<()> {
    let EventInsert {
        run_id,
        seq,
        event_type,
        node_instance_id,
        attempt_id,
        payload,
        now,
    } = event;
    connection.execute_raw(sql(
        "INSERT INTO run_events (id, run_id, seq, node_instance_id, attempt_id, event_type, schema_version, importance, payload_json, created_at) VALUES (?, ?, ?, ?, ?, ?, 1, 'critical', ?, ?)",
        vec![new_id("event").into(), run_id.into(), seq.into(), node_instance_id.map(String::from).into(), attempt_id.map(String::from).into(), event_type.into(), canonical::to_string(&payload)?.into(), now.into()],
    )).await?;
    Ok(())
}

async fn add_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    owner_kind: &str,
    owner_id: &str,
    role: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![object_id.into(), owner_kind.into(), owner_id.into(), role.into(), now.into()],
    )).await?;
    Ok(())
}
