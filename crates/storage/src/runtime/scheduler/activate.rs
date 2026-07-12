use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::Serialize;
use serde_json::{Map, Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphEdge, GraphNode},
    schema, selector,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{apply::load_revision, helpers::*},
};

use super::{
    events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run, finish_wakeup},
    llm_read_set::resolve_llm_reads,
    load::object_metadata,
    read_set::resolve_router_reads,
    router::create_control_snapshot,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValueRef<'a> {
    id: &'a str,
    content_hash: &'a str,
    encoding: &'static str,
    size_bytes: i64,
}

struct QueueHead {
    id: String,
    port: String,
    value_object_id: String,
    enqueue_seq: i64,
}

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
        if has_active(&transaction, run_id, node_id).await? {
            finish_wakeup(&transaction, wakeup_id).await?;
            transaction.commit().await?;
            return Ok(());
        }
        let heads = queue_heads(&transaction, run_id, node, &revision.definition.edges).await?;
        let Some(heads) = heads else {
            finish_and_settle(&transaction, wakeup_id, run_id, now).await?;
            transaction.commit().await?;
            return Ok(());
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
        let inputs_id = build_inputs(&transaction, node, &heads, &instance_id, now).await?;
        let activation_seq = allocate_activation_seq(&transaction, run_id, node_id).await?;
        transaction.execute_raw(sql(
            "INSERT INTO node_instances (id, run_id, node_id, activation_seq, status, graph_revision_id, inputs_object_id, created_at, updated_at) VALUES (?, ?, ?, ?, 'ready', ?, ?, ?, ?)",
            vec![instance_id.clone().into(), run_id.into(), node_id.into(), activation_seq.into(), revision_id.into(), inputs_id.clone().into(), now.into(), now.into()],
        )).await?;
        create_control_snapshot(&transaction, run_id, node, &instance_id, now).await?;
        transaction.execute_raw(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) SELECT ?, ?, 1, 0, 'start', 'queued', control_epoch, 0, ?, execution_manifest_object_id FROM graph_runs WHERE id = ?",
            vec![attempt_id.clone().into(), instance_id.clone().into(), format!("attempt:{instance_id}:1").into(), run_id.into()],
        )).await?;
        resolve_router_reads(&transaction, run_id, &attempt_id, node, now).await?;
        resolve_llm_reads(&transaction, run_id, &attempt_id, node, now).await?;
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

async fn queue_heads<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    edges: &[GraphEdge],
) -> StorageResult<Option<Vec<QueueHead>>> {
    let mut heads = Vec::new();
    for input in &node.inputs {
        let edge = edges
            .iter()
            .find(|edge| edge.to.node_id == node.id && edge.to.input == input.name)
            .ok_or_else(|| StorageError::Integrity("node input edge missing".into()))?;
        let row = connection.query_one_raw(sql(
            "SELECT id, value_object_id, enqueue_seq FROM edge_queue_values WHERE run_id = ? AND edge_id = ? AND consumed_at IS NULL ORDER BY enqueue_seq LIMIT 1",
            vec![run_id.into(), edge.id.clone().into()],
        )).await?;
        let Some(row) = row else {
            if matches!(&node.kind, DraftNodeKind::Merge { .. }) {
                continue;
            }
            return Ok(None);
        };
        heads.push(QueueHead {
            id: row.try_get("", "id")?,
            port: input.name.clone(),
            value_object_id: row.try_get("", "value_object_id")?,
            enqueue_seq: row.try_get("", "enqueue_seq")?,
        });
    }
    if matches!(&node.kind, DraftNodeKind::Merge { .. }) {
        let Some(head) = heads.into_iter().min_by_key(|head| head.enqueue_seq) else {
            return Ok(None);
        };
        return Ok(Some(vec![head]));
    }
    Ok(Some(heads))
}

async fn build_inputs<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    heads: &[QueueHead],
    instance_id: &str,
    now: i64,
) -> StorageResult<String> {
    let mut ports = Map::new();
    for head in heads {
        let input = node
            .inputs
            .iter()
            .find(|input| input.name == head.port)
            .ok_or_else(|| StorageError::Integrity("selected input port missing".into()))?;
        let raw: Value = load_object_json(connection, &head.value_object_id).await?;
        let selected = selector::select(&input.binding.selector, &raw, 100_000)
            .map_err(StorageError::InputContract)?;
        if let Some(spec) = &input.schema {
            schema::validate(spec, &selected)?;
        }
        let selected_bytes = canonical::to_vec(&selected)?;
        let selected_id = put_inline_object(connection, &selected_bytes, now).await?;
        let (raw_hash, raw_size) = object_metadata(connection, &head.value_object_id).await?;
        let selected_hash = canonical::hash_bytes(&selected_bytes);
        ports.insert(input.name.clone(), json!({
            "rawValue": ValueRef { id: &head.value_object_id, content_hash: &raw_hash, encoding: "canonical_json_v1", size_bytes: raw_size },
            "selectedValue": ValueRef { id: &selected_id, content_hash: &selected_hash, encoding: "canonical_json_v1", size_bytes: selected_bytes.len() as i64 },
            "queueItemIds": [head.id]
        }));
        add_object_ref(
            connection,
            &head.value_object_id,
            "node_instance",
            instance_id,
            &format!("raw_input:{}", input.name),
            now,
        )
        .await?;
        add_object_ref(
            connection,
            &selected_id,
            "node_instance",
            instance_id,
            &format!("selected_input:{}", input.name),
            now,
        )
        .await?;
    }
    put_inline_object(
        connection,
        &canonical::to_vec(&json!({"schemaVersion":1,"ports":ports}))?,
        now,
    )
    .await
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

async fn finish_and_settle<C: ConnectionTrait>(
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

async fn allocate_activation_seq<C: ConnectionTrait>(
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

async fn limits_exceeded<C: ConnectionTrait>(
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
