use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::Value;
use zhuangsheng_core::{
    application::context::CommitContextPatchCommand,
    canonical,
    graph::{DraftNodeKind, FinalValueSource, GraphNode, StaticContextWriteOp},
    selector,
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{
    StorageError, StorageResult,
    context::commit::commit_patch,
    graph::helpers::{load_object_json, sql},
};

use super::{
    attempt_state::AttemptState,
    emit::StoredValue,
    events::{Event, append_event},
    load::load_inputs,
};

pub(super) async fn apply<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    attempt_id: &str,
    node: &GraphNode,
    outputs: &BTreeMap<String, StoredValue>,
    now: i64,
) -> StorageResult<()> {
    let DraftNodeKind::Llm { config } = &node.kind else {
        return Ok(());
    };
    let Some(memory) = &config.memory else {
        return Ok(());
    };
    if memory.node.working_writes.is_empty() {
        return Ok(());
    }
    let base = connection.query_one_raw(sql(
        "SELECT b.context_id,b.branch_id,b.base_commit_id,r.context_id AS run_context,r.branch_id AS run_branch FROM node_static_write_bases b JOIN graph_runs r ON r.id=? WHERE b.node_instance_id=?",
        vec![state.run_id.clone().into(),state.node_instance_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::Integrity("static write base is missing".into()))?;
    let context_id: String = base.try_get("", "context_id")?;
    let branch_id: String = base.try_get("", "branch_id")?;
    if context_id != base.try_get::<String>("", "run_context")?
        || branch_id != base.try_get::<String>("", "run_branch")?
    {
        return Err(StorageError::Integrity(
            "static write base crossed the run context".into(),
        ));
    }
    let inputs = load_inputs(connection, node, &state.inputs_object_id).await?;
    let mut operations = Vec::with_capacity(memory.node.working_writes.len());
    for write in &memory.node.working_writes {
        let value = match &write.value_from {
            Some(source) => {
                Some(resolve_value(connection, attempt_id, config, &inputs, outputs, source).await?)
            }
            None => None,
        };
        operations.push(match write.op {
            StaticContextWriteOp::Add => JsonPatchOp::Add {
                path: write.path.clone(),
                value: value.expect("validated add selector"),
            },
            StaticContextWriteOp::Replace => JsonPatchOp::Replace {
                path: write.path.clone(),
                value: value.expect("validated replace selector"),
            },
            StaticContextWriteOp::Append => JsonPatchOp::Append {
                path: write.path.clone(),
                element_id: element_id(&state.node_instance_id, &write.id),
                value: value.expect("validated append selector"),
            },
            StaticContextWriteOp::Remove => JsonPatchOp::Remove {
                path: write.path.clone(),
            },
        });
    }
    let commit = commit_patch(
        connection,
        &CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: context_id,
                lineage_key: branch_id,
                base_commit_id: base.try_get("", "base_commit_id")?,
                operation_id: format!("static-write/v1:{}", state.node_instance_id),
                ops: operations,
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::Node,
                    id: Some(node.id.clone()),
                },
            },
            origin_run_id: Some(state.run_id.clone()),
            origin_node_instance_id: Some(state.node_instance_id.clone()),
        },
        now,
    )
    .await?;
    append_event(connection, Event {
        run_id: &state.run_id, event_type: "state.patch.committed", importance: "critical",
        node_instance_id: Some(&state.node_instance_id), attempt_id: Some(attempt_id),
        payload: serde_json::json!({"schemaVersion":1,"commitId":commit.id,"operationId":commit.operation_id}),
        now,
    }).await?;
    Ok(())
}

async fn resolve_value<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    config: &zhuangsheng_core::graph::LlmNodeConfig,
    inputs: &BTreeMap<String, Value>,
    outputs: &BTreeMap<String, StoredValue>,
    source: &zhuangsheng_core::graph::FinalValueSelector,
) -> StorageResult<Value> {
    let raw = match source.source {
        FinalValueSource::Input => inputs.get(&source.source_name).cloned(),
        FinalValueSource::Output => match outputs.get(&source.source_name) {
            Some(value) => Some(load_object_json(connection, &value.id).await?),
            None => None,
        },
        FinalValueSource::Binding => {
            let read = config.memory.as_ref().and_then(|memory| {
                memory
                    .node
                    .reads
                    .iter()
                    .find(|read| read.alias == source.source_name)
            });
            match read {
                Some(read) => Some(load_binding(connection, attempt_id, &read.id).await?),
                None => None,
            }
        }
    }
    .ok_or_else(|| {
        StorageError::InputContract(format!(
            "static write source '{}' is missing",
            source.source_name
        ))
    })?;
    selector::select(&source.selector, &raw, 100_000).map_err(StorageError::InputContract)
}

async fn load_binding<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    binding_id: &str,
) -> StorageResult<Value> {
    let row = connection.query_one_raw(sql(
        "SELECT envelope_object_id,result_digest FROM node_bound_read_results WHERE node_attempt_id=? AND binding_id=?",
        vec![attempt_id.into(),binding_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("static write binding is missing".into()))?;
    let result: Value = load_object_json(
        connection,
        &row.try_get::<String>("", "envelope_object_id")?,
    )
    .await?;
    let envelope = result.get("envelope").cloned().ok_or_else(|| {
        StorageError::Integrity("static write binding envelope is missing".into())
    })?;
    if canonical::hash(&envelope)? != row.try_get::<String>("", "result_digest")? {
        return Err(StorageError::Integrity(
            "static write binding digest mismatch".into(),
        ));
    }
    Ok(envelope)
}

fn element_id(instance_id: &str, write_id: &str) -> String {
    let mut bytes = b"static-write/v1\0".to_vec();
    bytes.extend_from_slice(instance_id.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(write_id.as_bytes());
    canonical::hash_bytes(&bytes)
}
