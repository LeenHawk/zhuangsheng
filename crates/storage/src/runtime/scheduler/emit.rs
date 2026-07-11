use std::collections::{BTreeMap, HashSet};

use sea_orm::ConnectionTrait;
use serde::Serialize;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode, OutputCollection, RunLimits},
    schema,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

use super::{
    events::{Event, add_object_ref, append_event, enqueue_wakeup},
    load::load_object_id_for_port,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredValue {
    pub id: String,
    pub content_hash: String,
    pub encoding: &'static str,
    pub size_bytes: i64,
}

pub(super) async fn prepare_outputs<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    outputs: &BTreeMap<String, Value>,
    output_order: Option<&[String]>,
    limits: &RunLimits,
    now: i64,
) -> StorageResult<BTreeMap<String, StoredValue>> {
    let order: Vec<_> = output_order.map_or_else(
        || node.outputs.iter().map(|port| port.name.clone()).collect(),
        <[String]>::to_vec,
    );
    let declared: HashSet<_> = node.outputs.iter().map(|port| port.name.as_str()).collect();
    let mut unique = HashSet::new();
    if outputs.len() != order.len()
        || order.is_empty() && matches!(&node.kind, DraftNodeKind::Router { .. })
        || order.iter().any(|name| {
            !declared.contains(name.as_str()) || !outputs.contains_key(name) || !unique.insert(name)
        })
    {
        return Err(StorageError::InputContract(
            "node output ports do not match applied graph".into(),
        ));
    }
    let mut stored = BTreeMap::new();
    for name in order {
        let port = node
            .outputs
            .iter()
            .find(|port| port.name == name)
            .ok_or_else(|| StorageError::Integrity("output port missing".into()))?;
        let value = &outputs[&name];
        if let Some(spec) = &port.schema {
            schema::validate(spec, value)?;
        }
        let bytes = canonical::to_vec(value)?;
        if bytes.len() as u64 > limits.max_value_bytes {
            return Err(StorageError::InputContract(
                "node output exceeds value limit".into(),
            ));
        }
        stored.insert(
            name,
            StoredValue {
                id: put_inline_object(connection, &bytes, now).await?,
                content_hash: canonical::hash_bytes(&bytes),
                encoding: "canonical_json_v1",
                size_bytes: bytes.len() as i64,
            },
        );
    }
    Ok(stored)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn emit_edges<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    instance_id: &str,
    node: &GraphNode,
    revision: &zhuangsheng_core::graph::AppliedGraphDefinition,
    values: &BTreeMap<String, StoredValue>,
    output_order: Option<&[String]>,
    limits: &RunLimits,
    now: i64,
) -> StorageResult<()> {
    let mut edges: Vec<_> = revision
        .edges
        .iter()
        .filter(|edge| edge.from.node_id == node.id && values.contains_key(&edge.from.output))
        .collect();
    let positions: BTreeMap<_, _> = output_order
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(index, port)| (port.as_str(), index))
        .collect();
    edges.sort_by(|left, right| {
        positions
            .get(left.from.output.as_str())
            .unwrap_or(&usize::MAX)
            .cmp(
                positions
                    .get(right.from.output.as_str())
                    .unwrap_or(&usize::MAX),
            )
            .then(left.from.output.cmp(&right.from.output))
            .then(left.id.cmp(&right.id))
    });
    if edges.is_empty() {
        return Ok(());
    }
    let counters = connection.query_one(sql(
        "SELECT next_enqueue_seq, total_queue_values, pending_queue_values FROM run_execution_counters WHERE run_id = ?",
        vec![run_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let first_seq: i64 = counters.try_get("", "next_enqueue_seq")?;
    let total: i64 = counters.try_get("", "total_queue_values")?;
    let pending: i64 = counters.try_get("", "pending_queue_values")?;
    let count = edges.len() as i64;
    if total.saturating_add(count) as u64 > limits.max_total_queue_values
        || pending.saturating_add(count) as u64 > limits.max_pending_queue_values
    {
        return Err(StorageError::InputContract(
            "run queue limit exceeded".into(),
        ));
    }
    let updated = connection.execute(sql(
        "UPDATE run_execution_counters SET next_enqueue_seq = next_enqueue_seq + ?, total_queue_values = total_queue_values + ?, pending_queue_values = pending_queue_values + ? WHERE run_id = ? AND next_enqueue_seq = ?",
        vec![count.into(), count.into(), count.into(), run_id.into(), first_seq.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_enqueue_sequence"));
    }
    let mut downstream = HashSet::new();
    for (index, edge) in edges.into_iter().enumerate() {
        let value = values
            .get(&edge.from.output)
            .ok_or_else(|| StorageError::Integrity("edge output value missing".into()))?;
        let queue_id = new_id("queue");
        let seq = first_seq + index as i64;
        connection.execute(sql(
            "INSERT INTO edge_queue_values (id, run_id, edge_id, enqueue_seq, producer_instance_id, producer_emission_index, value_object_id, created_at) VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
            vec![queue_id.clone().into(), run_id.into(), edge.id.clone().into(), seq.into(), instance_id.into(), value.id.clone().into(), now.into()],
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
            run_id,
            event_type: "edge.value.enqueued",
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: None,
            payload: json!({"schemaVersion":1,"queueValueId":queue_id,"edgeId":edge.id,"enqueueSeq":seq}),
            now,
        }).await?;
        if downstream.insert(edge.to.node_id.clone()) {
            enqueue_wakeup(
                connection,
                run_id,
                Some(&edge.to.node_id),
                "node_maybe_ready",
                event_seq,
                &format!("edge-ready:{queue_id}:{}", edge.to.node_id),
                now,
            )
            .await?;
        }
    }
    Ok(())
}

pub(super) async fn ensure_edge_capacity<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    revision: &zhuangsheng_core::graph::AppliedGraphDefinition,
    values: &BTreeMap<String, StoredValue>,
    limits: &RunLimits,
) -> StorageResult<()> {
    let count = revision
        .edges
        .iter()
        .filter(|edge| edge.from.node_id == node.id && values.contains_key(&edge.from.output))
        .count() as i64;
    if count == 0 {
        return Ok(());
    }
    let counters = connection.query_one(sql(
        "SELECT total_queue_values, pending_queue_values FROM run_execution_counters WHERE run_id = ?",
        vec![run_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let total: i64 = counters.try_get("", "total_queue_values")?;
    let pending: i64 = counters.try_get("", "pending_queue_values")?;
    if total.saturating_add(count) as u64 > limits.max_total_queue_values
        || pending.saturating_add(count) as u64 > limits.max_pending_queue_values
    {
        return Err(StorageError::InputContract(
            "run queue limit exceeded".into(),
        ));
    }
    Ok(())
}

pub(super) async fn commit_run_output<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    instance_id: &str,
    inputs_object_id: &str,
    node: &GraphNode,
    revision: &zhuangsheng_core::graph::AppliedGraphDefinition,
    now: i64,
) -> StorageResult<()> {
    let DraftNodeKind::Output { output_key } = &node.kind else {
        return Ok(());
    };
    let contract = revision
        .output_contract
        .iter()
        .find(|item| item.key == *output_key)
        .ok_or_else(|| StorageError::Integrity("output contract missing".into()))?;
    let input_port = &node.inputs[0].name;
    let object_id = load_object_id_for_port(connection, inputs_object_id, input_port).await?;
    let value: Value = load_object_json(connection, &object_id).await?;
    if let Some(spec) = &contract.schema {
        schema::validate(spec, &value)?;
    }
    let mode = match contract.collection {
        OutputCollection::Single => "single",
        OutputCollection::Append => "append",
    };
    if mode == "single"
        && connection
            .query_one(sql(
                "SELECT 1 AS present FROM run_output_values WHERE run_id = ? AND output_key = ?",
                vec![run_id.into(), output_key.clone().into()],
            ))
            .await?
            .is_some()
    {
        return Err(StorageError::InputContract(
            "single run output already exists".into(),
        ));
    }
    let row = connection
        .query_one(sql(
            "SELECT next_output_seq FROM run_execution_counters WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("run output counter missing".into()))?;
    let seq: i64 = row.try_get("", "next_output_seq")?;
    let updated = connection.execute(sql(
        "UPDATE run_execution_counters SET next_output_seq = next_output_seq + 1 WHERE run_id = ? AND next_output_seq = ?",
        vec![run_id.into(), seq.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_output_sequence"));
    }
    let output_id = new_id("runout");
    connection.execute(sql(
        "INSERT INTO run_output_values (id, run_id, output_key, collection_mode, output_seq, node_instance_id, value_object_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        vec![output_id.clone().into(), run_id.into(), output_key.clone().into(), mode.into(), seq.into(), instance_id.into(), object_id.clone().into(), now.into()],
    )).await?;
    add_object_ref(
        connection,
        &object_id,
        "run_output",
        &output_id,
        "value",
        now,
    )
    .await?;
    append_event(connection, Event {
        run_id,
        event_type: "run.output.committed",
        importance: "critical",
        node_instance_id: Some(instance_id),
        attempt_id: None,
        payload: json!({"schemaVersion":1,"outputKey":output_key,"collection":mode,"outputSeq":seq,"valueRef":object_id}),
        now,
    }).await?;
    Ok(())
}
