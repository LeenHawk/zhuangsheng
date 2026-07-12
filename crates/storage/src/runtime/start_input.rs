use sea_orm::ConnectionTrait;
use serde::Serialize;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode},
    schema, selector,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ValueRefEnvelope {
    pub id: String,
    pub content_hash: String,
    pub encoding: &'static str,
    pub size_bytes: u64,
}

pub(super) struct PreparedInput {
    pub node_id: String,
    pub instance_id: String,
    pub attempt_id: String,
    pub inputs_object_id: String,
    pub selected_object_id: String,
}

pub(super) async fn prepare_input_nodes<C: ConnectionTrait>(
    connection: &C,
    nodes: &[GraphNode],
    input: &Value,
    input_ref: &ValueRefEnvelope,
    max_value_bytes: u64,
    now: i64,
) -> StorageResult<Vec<PreparedInput>> {
    let mut prepared = Vec::new();
    for node in nodes.iter().filter(|node| node.is_entry) {
        let DraftNodeKind::Input { run_input_selector } = &node.kind else {
            return Err(StorageError::Integrity(
                "entry node is not InputNode".into(),
            ));
        };
        let selected = selector::select(run_input_selector, input, 100_000)
            .map_err(StorageError::InputContract)?;
        if let Some(spec) = node.outputs[0].schema.as_ref() {
            schema::validate(spec, &selected)?;
        }
        let bytes = canonical::to_vec(&selected)?;
        if bytes.len() as u64 > max_value_bytes {
            return Err(StorageError::InputContract(
                "selected input exceeds graph value limit".into(),
            ));
        }
        let selected_id = put_inline_object(connection, &bytes, now).await?;
        let selected_ref = ValueRefEnvelope {
            id: selected_id.clone(),
            content_hash: canonical::hash_bytes(&bytes),
            encoding: "canonical_json_v1",
            size_bytes: bytes.len() as u64,
        };
        let inputs = canonical::to_vec(&json!({
            "schemaVersion":1,
            "runInput":input_ref,
            "sourceOutput":{"port":node.outputs[0].name,"value":selected_ref}
        }))?;
        prepared.push(PreparedInput {
            node_id: node.id.clone(),
            instance_id: new_id("nodeinst"),
            attempt_id: new_id("attempt"),
            inputs_object_id: put_inline_object(connection, &inputs, now).await?,
            selected_object_id: selected_id,
        });
    }
    Ok(prepared)
}
