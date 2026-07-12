use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::Value;
use zhuangsheng_core::graph::{DraftNodeKind, GraphNode};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

pub(super) async fn load_inputs<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    inputs_object_id: &str,
) -> StorageResult<BTreeMap<String, Value>> {
    let envelope: Value = load_object_json(connection, inputs_object_id).await?;
    match &node.kind {
        DraftNodeKind::Input { .. } => {
            let port = envelope
                .pointer("/sourceOutput/port")
                .and_then(Value::as_str)
                .ok_or_else(|| integrity("source output port missing"))?;
            let value_id = envelope
                .pointer("/sourceOutput/value/id")
                .and_then(Value::as_str)
                .ok_or_else(|| integrity("source output value ref missing"))?;
            Ok(BTreeMap::from([(
                port.into(),
                load_object_json(connection, value_id).await?,
            )]))
        }
        _ => {
            let ports = envelope
                .get("ports")
                .and_then(Value::as_object)
                .ok_or_else(|| integrity("activation inputs ports missing"))?;
            let mut result = BTreeMap::new();
            for (port, binding) in ports {
                let value_id = binding
                    .pointer("/selectedValue/id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| integrity("selected input value ref missing"))?;
                result.insert(port.clone(), load_object_json(connection, value_id).await?);
            }
            Ok(result)
        }
    }
}

pub(super) async fn load_object_id_for_port<C: ConnectionTrait>(
    connection: &C,
    inputs_object_id: &str,
    port: &str,
) -> StorageResult<String> {
    let envelope: Value = load_object_json(connection, inputs_object_id).await?;
    envelope
        .pointer(&format!("/ports/{}/selectedValue/id", escape(port)))
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| integrity("selected input value ref missing"))
}

pub(super) async fn object_metadata<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
) -> StorageResult<(String, i64)> {
    let row = connection
        .query_one_raw(sql(
            "SELECT content_hash, byte_size FROM content_objects WHERE id = ? AND lifecycle = 'live'",
            vec![object_id.into()],
        ))
        .await?
        .ok_or_else(|| integrity("value object unavailable"))?;
    Ok((
        row.try_get("", "content_hash")?,
        row.try_get("", "byte_size")?,
    ))
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn integrity(message: &str) -> StorageError {
    StorageError::Integrity(message.into())
}
