use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptStatus, EffectStatus, LlmLogicalCallStatus,
        LlmLoopCheckpoint, PrepareInitialModelCallCommand, PrepareModelCallCommand,
        PreparedInitialModelCall, PreparedModelCall,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
    runtime::compute_llm_read_set_digest,
};

use super::{
    model_ledger_helpers::{
        add_ref, classification_name, load_existing, persist_checkpoint, validate_prepare_fields,
    },
    validation::{
        CheckpointExpectation, load_ledger_context, validate_checkpoint,
        validate_node_attempt_fence, validate_operation,
    },
};

impl SqliteStore {
    pub async fn prepare_initial_model_call(
        &self,
        command: PrepareInitialModelCallCommand,
        now: i64,
    ) -> StorageResult<PreparedInitialModelCall> {
        validate_initial_fields(&command)?;
        let transaction = self.db.begin().await?;
        if command.originating_attempt_id != command.fence.invoking_node_attempt_id {
            return Err(StorageError::InvalidArgument(
                "initial model call originating attempt does not match its fence".into(),
            ));
        }
        validate_node_attempt_fence(&transaction, &command.node_instance_id, &command.fence)
            .await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        let authoritative_read_set =
            compute_llm_read_set_digest(&transaction, &command.originating_attempt_id).await?;
        if authoritative_read_set != command.read_set_digest {
            return Err(StorageError::InvalidArgument(
                "initial model call read-set digest mismatch".into(),
            ));
        }
        let transcript_bytes = canonical::to_vec(&json!({
            "schemaVersion":1,
            "items":command.transcript,
        }))?;
        let transcript_ref = put_inline_object(&transaction, &transcript_bytes, now).await?;
        let checkpoint = LlmLoopCheckpoint {
            schema_version: 1,
            node_instance_id: command.node_instance_id.clone(),
            last_updated_by_attempt_id: command.originating_attempt_id.clone(),
            graph_revision_id: context.graph_revision_id.clone(),
            registry_snapshot: command.registry_snapshot.clone(),
            context_snapshot_ref: context.execution_snapshot_object_id.clone(),
            read_set_digest: command.read_set_digest.clone(),
            model_call_no: 1,
            transcript_ref,
            continuation_ref: None,
            active_model_effect: Some(ActiveModelEffectCheckpoint {
                model_call_id: command.model_call_id.clone(),
                effect_id: command.effect_id.clone(),
                status: LlmLogicalCallStatus::Prepared,
                response_ref: None,
            }),
            active_count_effect: None,
            current_batch: Vec::new(),
            model_calls_used: 1,
            count_calls_used: 0,
            tool_calls_used: 0,
            effect_watermark: command.effect_attempt_id.clone(),
            wait_ids: Vec::new(),
            checksum: String::new(),
        }
        .seal()?;
        let prepare = PrepareModelCallCommand {
            model_call_id: command.model_call_id,
            effect_id: command.effect_id,
            effect_attempt_id: command.effect_attempt_id,
            node_instance_id: command.node_instance_id,
            originating_attempt_id: command.originating_attempt_id,
            fence: command.fence,
            call_no: 1,
            channel_id: command.channel_id,
            operation: command.operation,
            request_bytes: command.request_bytes,
            effect_kind: command.effect_kind,
            effect_classification: command.effect_classification,
            effect_operation_key: command.effect_operation_key,
            effect_idempotency_key: command.effect_idempotency_key,
            retry_policy: command.retry_policy,
            checkpoint: checkpoint.clone(),
        };
        let prepared = insert_initial(&transaction, &context, &prepare, now).await?;
        transaction.commit().await?;
        Ok(PreparedInitialModelCall {
            prepared,
            checkpoint,
        })
    }
}

fn validate_initial_fields(command: &PrepareInitialModelCallCommand) -> StorageResult<()> {
    if command.registry_snapshot.revision.trim().is_empty()
        || command.read_set_digest.trim().is_empty()
        || command.transcript.len() > 4096
    {
        return Err(StorageError::InvalidArgument(
            "initial model call snapshot fields are invalid".into(),
        ));
    }
    Ok(())
}

async fn insert_initial<C: ConnectionTrait>(
    connection: &C,
    context: &super::validation::LedgerContext,
    command: &PrepareModelCallCommand,
    now: i64,
) -> StorageResult<PreparedModelCall> {
    validate_prepare_fields(command)?;
    validate_operation(
        &context.snapshot.operation,
        &command.operation,
        &command.channel_id,
        context,
    )?;
    validate_checkpoint(
        &command.checkpoint,
        CheckpointExpectation {
            context,
            node_instance_id: &command.node_instance_id,
            updater_attempt_id: &command.originating_attempt_id,
            call_no: 1,
            model_call_id: &command.model_call_id,
            effect_id: &command.effect_id,
            effect_attempt_id: &command.effect_attempt_id,
            status: LlmLogicalCallStatus::Prepared,
            response_ref: None,
        },
    )?;
    let operation_json = canonical::to_string(&command.operation.operation_key)?;
    let request_digest = canonical::hash_bytes(&command.request_bytes);
    let retry_json = canonical::to_string(&command.retry_policy)?;
    if let Some(existing) = load_existing(
        connection,
        command,
        &operation_json,
        &request_digest,
        &retry_json,
    )
    .await?
    {
        return Ok(existing);
    }
    let count: i64 = connection
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM model_calls WHERE node_instance_id = ?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .expect("count query returns a row")
        .try_get("", "count")?;
    if count != 0 {
        return Err(StorageError::InvalidArgument(
            "initial model call requires an empty model-call ledger".into(),
        ));
    }
    let request_object_id = put_inline_object(connection, &command.request_bytes, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO model_calls (id, node_instance_id, originating_attempt_id, call_no, channel_id, channel_revision_id, model_id, operation_key_json, operation_taxonomy_version, adapter_decoder_version, request_object_id, status, started_at) VALUES (?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, 'prepared', ?)",
        vec![command.model_call_id.clone().into(), command.node_instance_id.clone().into(), command.originating_attempt_id.clone().into(), command.channel_id.clone().into(), command.operation.channel_revision_id.clone().into(), command.operation.model_id.clone().into(), operation_json.into(), i64::from(command.operation.operation_taxonomy_version).into(), i64::from(command.operation.adapter_decoder_version).into(), request_object_id.clone().into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO effects (id, node_instance_id, model_call_id, effect_kind, classification, operation_key, idempotency_key, retry_policy_json, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
        vec![command.effect_id.clone().into(), command.node_instance_id.clone().into(), command.model_call_id.clone().into(), command.effect_kind.clone().into(), classification_name(command.effect_classification).into(), command.effect_operation_key.clone().into(), command.effect_idempotency_key.clone().into(), retry_json.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, 1, 'prepared', ?)",
        vec![command.effect_attempt_id.clone().into(), command.effect_id.clone().into(), command.originating_attempt_id.clone().into(), request_object_id.clone().into()],
    )).await?;
    persist_checkpoint(connection, &command.checkpoint, now).await?;
    add_ref(
        connection,
        &request_object_id,
        "model_call",
        &command.model_call_id,
        "request",
        now,
    )
    .await?;
    add_ref(
        connection,
        &request_object_id,
        "effect_attempt",
        &command.effect_attempt_id,
        "request",
        now,
    )
    .await?;
    Ok(PreparedModelCall {
        model_call_id: command.model_call_id.clone(),
        effect_id: command.effect_id.clone(),
        effect_attempt_id: command.effect_attempt_id.clone(),
        model_status: LlmLogicalCallStatus::Prepared,
        effect_status: EffectStatus::Pending,
        attempt_status: EffectAttemptStatus::Prepared,
        replayed: false,
    })
}
