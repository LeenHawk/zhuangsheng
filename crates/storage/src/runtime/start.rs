use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::Serialize;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode},
    runtime::{RunContextCommand, RunView, StartRunCommand},
    schema, selector,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{apply::load_revision, helpers::*},
};

use super::{
    context::{bind_existing_context, create_temporary_context},
    persist::{RuntimeRows, add_run_refs, insert_run, insert_runtime_rows},
    query::load_run,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValueRefEnvelope {
    id: String,
    content_hash: String,
    encoding: &'static str,
    size_bytes: u64,
}

pub(super) struct PreparedInput {
    pub node_id: String,
    pub instance_id: String,
    pub attempt_id: String,
    pub inputs_object_id: String,
    pub selected_object_id: String,
}

impl SqliteStore {
    pub async fn start_run(&self, command: StartRunCommand) -> StorageResult<RunView> {
        if command.idempotency_key.trim().is_empty() {
            return Err(StorageError::InvalidArgument(
                "missing idempotency key".into(),
            ));
        }
        let input_bytes = canonical::to_vec(&command.input)?;
        let input_hash = canonical::hash_bytes(&input_bytes);
        let scope = format!(
            "workspace:local:graph-revisions:{}:runs",
            command.graph_revision_id
        );
        let digest = canonical::hash(&json!({
            "command": "start_run",
            "graphRevisionId": command.graph_revision_id,
            "inputHash": input_hash,
            "context": command.context,
            "deadlineAt": command.deadline_at,
        }))?;
        let run_id = new_id("run");
        let transaction = self.db.begin().await?;
        if let Some(run) =
            find_existing_run(&transaction, &scope, &command.idempotency_key, &digest).await?
        {
            transaction.commit().await?;
            return Ok(run);
        }

        let revision = load_revision(&transaction, &command.graph_revision_id).await?;
        if input_bytes.len() as u64 > revision.definition.limits.max_value_bytes {
            return Err(StorageError::InputContract(
                "run input exceeds graph value limit".into(),
            ));
        }
        if let Some(spec) = &revision.definition.run_input_schema {
            schema::validate(spec, &command.input)?;
        }
        let now = now_ms();
        let deadline_at = deadline(
            now,
            command.deadline_at,
            revision.definition.limits.max_run_wall_clock_ms,
        )?;
        let binding = match &command.context {
            RunContextCommand::Temporary => {
                create_temporary_context(&transaction, &run_id, now).await?
            }
            RunContextCommand::Existing {
                context_id,
                branch_id,
                expected_head_commit_id,
            } => {
                bind_existing_context(&transaction, context_id, branch_id, expected_head_commit_id)
                    .await?
            }
        };
        let input_object_id = put_inline_object(&transaction, &input_bytes, now).await?;
        let input_ref = ValueRefEnvelope {
            id: input_object_id.clone(),
            content_hash: input_hash,
            encoding: "canonical_json_v1",
            size_bytes: input_bytes.len() as u64,
        };
        let manifest = canonical::to_vec(&json!({
            "schemaVersion": 1,
            "graphRevisionId": revision.id,
            "graphContentHash": revision.content_hash,
            "operationTaxonomyVersion": revision.operation_taxonomy_version,
            "adapterDecoderVersion": revision.adapter_decoder_version,
        }))?;
        let manifest_id = put_inline_object(&transaction, &manifest, now).await?;
        let limits = canonical::to_vec(&revision.definition.limits)?;
        let limits_id = put_inline_object(&transaction, &limits, now).await?;
        let prepared = prepare_input_nodes(
            &transaction,
            &revision.definition.nodes,
            &command.input,
            &input_ref,
            revision.definition.limits.max_value_bytes,
            now,
        )
        .await?;

        insert_run(
            &transaction,
            &run_id,
            &scope,
            &command,
            &digest,
            &revision.content_hash,
            &manifest_id,
            &binding,
            &limits_id,
            &input_object_id,
            deadline_at,
            now,
        )
        .await?;
        insert_runtime_rows(
            &transaction,
            RuntimeRows {
                run_id: &run_id,
                graph_revision_id: &revision.id,
                nodes: &revision.definition.nodes,
                prepared: &prepared,
                manifest_id: &manifest_id,
                binding: &binding,
                now,
            },
        )
        .await?;
        add_run_refs(
            &transaction,
            &run_id,
            &input_object_id,
            &manifest_id,
            &limits_id,
            now,
        )
        .await?;
        transaction.commit().await?;
        load_run(&self.db, &run_id).await
    }
}

async fn prepare_input_nodes<C: ConnectionTrait>(
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
            "schemaVersion": 1,
            "runInput": input_ref,
            "sourceOutput": {
                "port": node.outputs[0].name,
                "value": selected_ref,
            }
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

fn deadline(now: i64, requested: Option<i64>, max_wall_ms: u64) -> StorageResult<i64> {
    if requested.is_some_and(|value| value <= now) {
        return Err(StorageError::InvalidArgument(
            "deadline must be in the future".into(),
        ));
    }
    let hard = now.saturating_add(i64::try_from(max_wall_ms).unwrap_or(i64::MAX));
    Ok(requested.map_or(hard, |value| value.min(hard)))
}

async fn find_existing_run<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<RunView>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT id, request_digest FROM graph_runs WHERE request_idempotency_scope = ? AND request_idempotency_key = ?",
            vec![scope.into(), key.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    let id: String = row.try_get("", "id")?;
    Ok(Some(load_run(connection, &id).await?))
}
