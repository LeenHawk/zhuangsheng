use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    graph::{DraftNodeKind, GraphEdge, GraphNode, InputSelector, RunLimits, canonical_join_key},
    schema, selector,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, sql},
};

use super::{
    activation_inputs::QueueHead,
    events::{Event, append_event},
    join_by_key_buffer::check_limits,
};

pub(super) enum JoinPreparation {
    NotReady,
    Ready(JoinTuple),
    Invalid(JoinInvalid),
    LimitExceeded(String),
}

pub(super) struct JoinTuple {
    pub heads: Vec<QueueHead>,
    pub key: Value,
}

pub(super) struct JoinInvalid {
    pub head: QueueHead,
    pub code: &'static str,
    pub safe_message: String,
}

struct IndexedItem {
    head: QueueHead,
    key_json: String,
}

struct PendingItem {
    input_index: usize,
    head: QueueHead,
}

pub(super) async fn prepare<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    edges: &[GraphEdge],
    limits: &RunLimits,
    now: i64,
) -> StorageResult<JoinPreparation> {
    let DraftNodeKind::JoinByKey {
        key_selectors,
        max_open_keys,
        max_buffered_per_key_per_port,
    } = &node.kind
    else {
        return Err(StorageError::Integrity(
            "join preparation used for another node".into(),
        ));
    };
    let mut pending = Vec::new();
    for (input_index, input) in node.inputs.iter().enumerate() {
        let edge = edges
            .iter()
            .find(|edge| edge.to.node_id == node.id && edge.to.input == input.name)
            .ok_or_else(|| StorageError::Integrity("join input edge missing".into()))?;
        let rows = connection.query_all_raw(sql(
            "SELECT q.id, q.value_object_id, q.enqueue_seq FROM edge_queue_values q LEFT JOIN coordination_buffer_items c ON c.queue_value_id = q.id WHERE q.run_id = ? AND q.edge_id = ? AND q.consumed_at IS NULL AND c.id IS NULL ORDER BY q.enqueue_seq",
            vec![run_id.into(), edge.id.clone().into()],
        )).await?;
        for row in rows {
            pending.push(PendingItem {
                input_index,
                head: QueueHead {
                    id: row.try_get("", "id")?,
                    port: input.name.clone(),
                    value_object_id: row.try_get("", "value_object_id")?,
                    enqueue_seq: row.try_get("", "enqueue_seq")?,
                },
            });
        }
    }
    pending.sort_by_key(|item| item.head.enqueue_seq);
    for item in pending {
        let input = &node.inputs[item.input_index];
        let head = item.head;
        let raw: Value = load_object_json(connection, &head.value_object_id).await?;
        let selected = match selector::select(&input.binding.selector, &raw, 100_000) {
            Ok(value) => value,
            Err(_) => return Ok(invalid_input(head)),
        };
        if let Some(spec) = &input.schema
            && schema::validate(spec, &selected).is_err()
        {
            return Ok(invalid_input(head));
        }
        let pointer = key_selectors
            .get(&input.name)
            .ok_or_else(|| StorageError::Integrity("join key selector missing".into()))?;
        let key = match selector::select(
            &InputSelector::JsonPointer {
                pointer: pointer.clone(),
            },
            &selected,
            1,
        )
        .ok()
        .and_then(|value| canonical_join_key(&value).ok())
        {
            Some(key) => key,
            None => {
                return Ok(JoinPreparation::Invalid(JoinInvalid {
                    head,
                    code: "join_key_invalid",
                    safe_message: "join key is missing or is not a supported scalar".into(),
                }));
            }
        };
        if let Some(message) = check_limits(
            connection,
            run_id,
            node,
            &input.name,
            &key.bytes,
            *max_open_keys,
            *max_buffered_per_key_per_port,
            limits,
        )
        .await?
        {
            return Ok(JoinPreparation::LimitExceeded(message));
        }
        let key_json = String::from_utf8(key.bytes.clone())
            .map_err(|_| StorageError::Integrity("canonical join key is not UTF-8".into()))?;
        connection.execute_raw(sql(
            "INSERT INTO coordination_buffer_items (id, run_id, node_id, input_port, queue_value_id, enqueue_seq, key_json, key_canonical, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'indexed', ?)",
            vec![new_id("coorditem").into(), run_id.into(), node.id.clone().into(), input.name.clone().into(), head.id.clone().into(), head.enqueue_seq.into(), key_json.into(), key.bytes.into(), now.into()],
        )).await?;
        connection.execute_raw(sql(
            "UPDATE run_execution_counters SET coordinator_buffered_values = coordinator_buffered_values + 1 WHERE run_id = ?",
            vec![run_id.into()],
        )).await?;
        append_event(connection, Event {
            run_id,
            event_type: "coordination.join_item_indexed",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1,"nodeId":node.id,"inputPort":input.name,"queueValueId":head.id,"enqueueSeq":head.enqueue_seq,"key":key.value}),
            now,
        }).await?;
    }
    select_ready(connection, run_id, node).await
}

fn invalid_input(head: QueueHead) -> JoinPreparation {
    JoinPreparation::Invalid(JoinInvalid {
        head,
        code: "input_contract_violation",
        safe_message: "node input does not satisfy its selector or schema".into(),
    })
}

async fn select_ready<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
) -> StorageResult<JoinPreparation> {
    let rows = connection.query_all_raw(sql(
        "SELECT c.input_port, c.key_json, c.key_canonical, q.id, q.value_object_id, q.enqueue_seq FROM coordination_buffer_items c JOIN edge_queue_values q ON q.id = c.queue_value_id WHERE c.run_id = ? AND c.node_id = ? AND c.status = 'indexed' AND q.consumed_at IS NULL ORDER BY q.enqueue_seq",
        vec![run_id.into(), node.id.clone().into()],
    )).await?;
    let mut groups: BTreeMap<Vec<u8>, BTreeMap<String, Vec<IndexedItem>>> = BTreeMap::new();
    for row in rows {
        let port: String = row.try_get("", "input_port")?;
        let key: Vec<u8> = row.try_get("", "key_canonical")?;
        groups
            .entry(key)
            .or_default()
            .entry(port.clone())
            .or_default()
            .push(IndexedItem {
                head: QueueHead {
                    id: row.try_get("", "id")?,
                    port,
                    value_object_id: row.try_get("", "value_object_id")?,
                    enqueue_seq: row.try_get("", "enqueue_seq")?,
                },
                key_json: row.try_get("", "key_json")?,
            });
    }
    let mut best: Option<(i64, Vec<u8>, Vec<QueueHead>, Value)> = None;
    for (key, ports) in groups {
        let Some(first) = node
            .inputs
            .first()
            .and_then(|input| ports.get(&input.name))
            .and_then(|items| items.first())
        else {
            continue;
        };
        let mut heads = Vec::with_capacity(node.inputs.len());
        for input in &node.inputs {
            let Some(item) = ports.get(&input.name).and_then(|items| items.first()) else {
                heads.clear();
                break;
            };
            heads.push(item.head.clone());
        }
        if heads.len() != node.inputs.len() {
            continue;
        }
        let ready_seq = heads.iter().map(|head| head.enqueue_seq).max().unwrap_or(0);
        let value: Value = serde_json::from_str(&first.key_json)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
        if best
            .as_ref()
            .is_none_or(|current| (ready_seq, &key) < (current.0, &current.1))
        {
            best = Some((ready_seq, key, heads, value));
        }
    }
    Ok(
        best.map_or(JoinPreparation::NotReady, |(_, _, heads, key)| {
            JoinPreparation::Ready(JoinTuple { heads, key })
        }),
    )
}

pub(super) async fn consume<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    tuple: &JoinTuple,
    instance_id: &str,
    attempt_id: &str,
    now: i64,
) -> StorageResult<()> {
    for head in &tuple.heads {
        let updated = connection.execute_raw(sql(
            "UPDATE coordination_buffer_items SET status = 'consumed', consumed_by_instance_id = ?, terminal_at = ? WHERE queue_value_id = ? AND status = 'indexed'",
            vec![instance_id.into(), now.into(), head.id.clone().into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("join_buffer_item"));
        }
    }
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET coordinator_buffered_values = coordinator_buffered_values - ? WHERE run_id = ?",
        vec![(tuple.heads.len() as i64).into(), run_id.into()],
    )).await?;
    append_event(connection, Event {
        run_id,
        event_type: "coordination.join_tuple_selected",
        importance: "critical",
        node_instance_id: Some(instance_id),
        attempt_id: Some(attempt_id),
        payload: json!({"schemaVersion":1,"nodeId":node.id,"key":tuple.key,"tupleReadySeq":tuple.heads.iter().map(|head| head.enqueue_seq).max(),"items":tuple.heads.iter().map(|head| json!({"inputPort":head.port,"queueValueId":head.id,"enqueueSeq":head.enqueue_seq})).collect::<Vec<_>>() }),
        now,
    }).await?;
    Ok(())
}
