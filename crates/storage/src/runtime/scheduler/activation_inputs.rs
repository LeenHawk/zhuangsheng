use sea_orm::ConnectionTrait;
use serde::Serialize;
use serde_json::{Map, Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphEdge, GraphNode},
    schema, selector,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
};

use super::{events::add_object_ref, load::object_metadata};

#[derive(Clone)]
pub(super) struct QueueHead {
    pub id: String,
    pub port: String,
    pub value_object_id: String,
    pub enqueue_seq: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ValueRef<'a> {
    id: &'a str,
    content_hash: &'a str,
    encoding: &'static str,
    size_bytes: i64,
}

pub(super) async fn queue_heads<C: ConnectionTrait>(
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

pub(super) async fn build_inputs<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    heads: &[QueueHead],
    instance_id: &str,
    join_key: Option<&Value>,
    now: i64,
) -> StorageResult<String> {
    let mut validated = Vec::with_capacity(heads.len());
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
        validated.push((input, head, canonical::to_vec(&selected)?));
    }
    let mut ports = Map::new();
    for (input, head, selected_bytes) in validated {
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
    let coordination = join_key.map(|key| json!({"joinKey":key}));
    put_inline_object(
        connection,
        &canonical::to_vec(&json!({"schemaVersion":1,"ports":ports,"coordination":coordination}))?,
        now,
    )
    .await
}
