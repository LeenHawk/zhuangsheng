use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    runtime::{RunContextCommand, StartRunCommand},
    schema,
};

use crate::{
    StorageError, StorageResult,
    graph::{apply::load_revision, helpers::put_inline_object},
};

use super::{
    context::{bind_existing_context, create_temporary_context},
    persist::{RuntimeRows, add_run_refs, insert_run, insert_runtime_rows},
    start_input::{ValueRefEnvelope, prepare_input_nodes},
};

pub(crate) struct RunIdentity {
    pub scope: String,
    pub digest: String,
    input_bytes: Vec<u8>,
    input_hash: String,
}

pub(crate) async fn insert_new_run<C: ConnectionTrait>(
    connection: &C,
    command: &StartRunCommand,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let identity = run_identity(command)?;
    let revision = load_revision(connection, &command.graph_revision_id).await?;
    if identity.input_bytes.len() as u64 > revision.definition.limits.max_value_bytes {
        return Err(StorageError::InputContract(
            "run input exceeds graph value limit".into(),
        ));
    }
    if let Some(spec) = &revision.definition.run_input_schema {
        schema::validate(spec, &command.input)?;
    }
    let deadline_at = deadline(
        now,
        command.deadline_at,
        revision.definition.limits.max_run_wall_clock_ms,
    )?;
    let binding = match &command.context {
        RunContextCommand::Temporary => create_temporary_context(connection, run_id, now).await?,
        RunContextCommand::Existing {
            context_id,
            branch_id,
            expected_head_commit_id,
        } => {
            bind_existing_context(connection, context_id, branch_id, expected_head_commit_id)
                .await?
        }
    };
    let input_object_id = put_inline_object(connection, &identity.input_bytes, now).await?;
    let input_ref = ValueRefEnvelope {
        id: input_object_id.clone(),
        content_hash: identity.input_hash,
        encoding: "canonical_json_v1",
        size_bytes: identity.input_bytes.len() as u64,
    };
    let manifest = canonical::to_vec(&json!({
        "schemaVersion":1,
        "graphRevisionId":revision.id,
        "graphContentHash":revision.content_hash,
        "operationTaxonomyVersion":revision.operation_taxonomy_version,
        "adapterDecoderVersion":revision.adapter_decoder_version,
    }))?;
    let manifest_id = put_inline_object(connection, &manifest, now).await?;
    let limits_id = put_inline_object(
        connection,
        &canonical::to_vec(&revision.definition.limits)?,
        now,
    )
    .await?;
    let prepared = prepare_input_nodes(
        connection,
        &revision.definition.nodes,
        &command.input,
        &input_ref,
        revision.definition.limits.max_value_bytes,
        now,
    )
    .await?;
    insert_run(
        connection,
        run_id,
        &identity.scope,
        command,
        &identity.digest,
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
        connection,
        RuntimeRows {
            run_id,
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
        connection,
        run_id,
        &input_object_id,
        &manifest_id,
        &limits_id,
        now,
    )
    .await
}

pub(crate) fn run_identity(command: &StartRunCommand) -> StorageResult<RunIdentity> {
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
        "command":"start_run",
        "graphRevisionId":command.graph_revision_id,
        "inputHash":input_hash,
        "context":command.context,
        "deadlineAt":command.deadline_at,
    }))?;
    Ok(RunIdentity {
        scope,
        digest,
        input_bytes,
        input_hash,
    })
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
