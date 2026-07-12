use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        LlmLogicalCallStatus, PrepareLlmOutputRepairCommand, PreparedLlmOutputRepair,
        ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole, validate_transcript_ir},
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    runtime::{Event, append_event},
};

use super::{
    model_ledger_helpers::{add_ref, persist_checkpoint},
    validation::{
        CheckpointExpectation, load_ledger_context, validate_checkpoint,
        validate_node_attempt_fence,
    },
};

impl SqliteStore {
    pub async fn prepare_llm_output_repair(
        &self,
        command: PrepareLlmOutputRepairCommand,
        now: i64,
    ) -> StorageResult<PreparedLlmOutputRepair> {
        validate_command(&command)?;
        let request_digest = repair_request_digest(&command)?;
        let transaction = self.db.begin().await?;
        validate_node_attempt_fence(&transaction, &command.node_instance_id, &command.fence)
            .await?;
        if let Some(replayed) = load_replay(&transaction, &command, &request_digest).await? {
            transaction.commit().await?;
            return Ok(replayed);
        }
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        let source = transaction
            .query_one_raw(sql(
                "SELECT mc.call_no, mc.status AS model_status, mc.response_object_id, e.id AS effect_id, e.status AS effect_status, ea.id AS effect_attempt_id, ea.status AS attempt_status FROM model_calls mc JOIN effects e ON e.model_call_id = mc.id JOIN effect_attempts ea ON ea.effect_id = e.id WHERE mc.id = ? AND mc.node_instance_id = ? ORDER BY ea.attempt_no DESC LIMIT 1",
                vec![command.source_model_call_id.clone().into(), command.node_instance_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::NotFound {
                kind: "model_call",
                id: command.source_model_call_id.clone(),
            })?;
        let response_ref: Option<String> = source.try_get("", "response_object_id")?;
        let response_ref = response_ref
            .ok_or_else(|| StorageError::Integrity("repair source response is missing".into()))?;
        if source.try_get::<String>("", "model_status")? != "completed"
            || source.try_get::<String>("", "effect_status")? != "succeeded"
            || source.try_get::<String>("", "attempt_status")? != "succeeded"
            || !command.checkpoint.current_batch.is_empty()
        {
            return Err(StorageError::Conflict("llm_output_repair_source"));
        }
        let call_no = u64::try_from(source.try_get::<i64>("", "call_no")?)
            .map_err(|_| StorageError::Integrity("invalid repair source call number".into()))?;
        let effect_id: String = source.try_get("", "effect_id")?;
        let effect_attempt_id: String = source.try_get("", "effect_attempt_id")?;
        validate_checkpoint(
            &command.checkpoint,
            CheckpointExpectation {
                context: &context,
                node_instance_id: &command.node_instance_id,
                updater_attempt_id: &command.fence.invoking_node_attempt_id,
                call_no,
                model_call_id: &command.source_model_call_id,
                effect_id: &effect_id,
                effect_attempt_id: &effect_attempt_id,
                status: LlmLogicalCallStatus::Completed,
                response_ref: Some(&response_ref),
            },
        )?;
        let count = repair_count(&transaction, &command.node_instance_id).await?;
        let repair_no = count
            .checked_add(1)
            .ok_or_else(|| StorageError::Integrity("output repair count overflow".into()))?;
        let limit =
            context.snapshot.limits.max_output_repairs.ok_or_else(|| {
                StorageError::Integrity("output repair limit is not pinned".into())
            })?;
        if repair_no > limit {
            return Err(StorageError::InvalidArgument(
                "LLM output repair limit exceeded".into(),
            ));
        }
        let mut transcript: Vec<LlmTurnItemIr> =
            load_transcript(&transaction, &command.checkpoint.transcript_ref).await?;
        transcript.push(command.instruction.clone());
        validate_transcript_ir(&transcript).map_err(|error| {
            StorageError::InvalidArgument(format!("invalid repair transcript: {}", error.message))
        })?;
        let transcript_ref = put_inline_object(
            &transaction,
            &canonical::to_vec(&json!({"schemaVersion":1,"items":transcript}))?,
            now,
        )
        .await?;
        let error_object_id = put_inline_object(
            &transaction,
            &canonical::to_vec(&json!({
                "schemaVersion":1,
                "errorCode":command.error_code,
                "extractedBytesDigest":command.extracted_bytes_digest,
            }))?,
            now,
        )
        .await?;
        let instruction_object_id =
            put_inline_object(&transaction, &canonical::to_vec(&command.instruction)?, now).await?;
        let mut checkpoint = command.checkpoint.clone();
        checkpoint.transcript_ref = transcript_ref;
        checkpoint.last_updated_by_attempt_id = command.fence.invoking_node_attempt_id.clone();
        checkpoint.effect_watermark = format!("outputrepair:{}", command.repair_id);
        checkpoint = checkpoint.seal()?;
        transaction
            .execute_raw(sql(
                "INSERT INTO llm_output_repairs (id, node_instance_id, repair_no, source_model_call_id, originating_attempt_id, extracted_bytes_digest, error_code, error_object_id, instruction_object_id, request_digest, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    command.repair_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    i64::try_from(repair_no).map_err(|_| StorageError::InvalidArgument("repair number is too large".into()))?.into(),
                    command.source_model_call_id.clone().into(),
                    command.fence.invoking_node_attempt_id.clone().into(),
                    command.extracted_bytes_digest.clone().into(),
                    command.error_code.clone().into(),
                    error_object_id.clone().into(),
                    instruction_object_id.clone().into(),
                    request_digest.into(),
                    now.into(),
                ],
            ))
            .await?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        add_ref(
            &transaction,
            &error_object_id,
            "llm_output_repair",
            &command.repair_id,
            "error",
            now,
        )
        .await?;
        add_ref(
            &transaction,
            &instruction_object_id,
            "llm_output_repair",
            &command.repair_id,
            "instruction",
            now,
        )
        .await?;
        append_repair_event(&transaction, &command, repair_no, now).await?;
        transaction.commit().await?;
        Ok(PreparedLlmOutputRepair {
            repair_id: command.repair_id,
            repair_no,
            checkpoint,
            transcript,
            replayed: false,
        })
    }
}

fn validate_command(command: &PrepareLlmOutputRepairCommand) -> StorageResult<()> {
    let instruction_text = match &command.instruction {
        LlmTurnItemIr::Message {
            id,
            role: MessageRole::User,
            content,
            ..
        } if !id.is_empty() && id.len() <= 256 && content.len() == 1 => match &content[0] {
            LlmContentPartIr::Text { text } => Some(text),
            _ => None,
        },
        _ => None,
    };
    if command.repair_id.is_empty()
        || command.repair_id.len() > 256
        || command.node_instance_id.is_empty()
        || command.source_model_call_id.is_empty()
        || command.extracted_bytes_digest.is_empty()
        || command.extracted_bytes_digest.len() > 128
        || command.error_code.is_empty()
        || command.error_code.len() > 128
        || instruction_text.is_none_or(|text| text.is_empty() || text.len() > 16 * 1024)
    {
        return Err(StorageError::InvalidArgument(
            "LLM output repair command is outside supported bounds".into(),
        ));
    }
    Ok(())
}

fn repair_request_digest(command: &PrepareLlmOutputRepairCommand) -> StorageResult<String> {
    canonical::hash(&json!({
        "repairId":command.repair_id,
        "nodeInstanceId":command.node_instance_id,
        "sourceModelCallId":command.source_model_call_id,
        "extractedBytesDigest":command.extracted_bytes_digest,
        "errorCode":command.error_code,
        "instruction":command.instruction,
    }))
    .map_err(Into::into)
}

async fn repair_count<C: ConnectionTrait>(connection: &C, node_id: &str) -> StorageResult<u64> {
    let count: i64 = connection
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM llm_output_repairs WHERE node_instance_id = ?",
            vec![node_id.into()],
        ))
        .await?
        .expect("count query returns a row")
        .try_get("", "count")?;
    u64::try_from(count).map_err(|_| StorageError::Integrity("invalid repair count".into()))
}

async fn load_transcript<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
) -> StorageResult<Vec<LlmTurnItemIr>> {
    let value: serde_json::Value = load_object_json(connection, object_id).await?;
    serde_json::from_value(
        value
            .get("items")
            .cloned()
            .ok_or_else(|| StorageError::Integrity("repair transcript items are missing".into()))?,
    )
    .map_err(|error| StorageError::Integrity(error.to_string()))
}

async fn load_replay<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareLlmOutputRepairCommand,
    digest: &str,
) -> StorageResult<Option<PreparedLlmOutputRepair>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT id, node_instance_id, repair_no, originating_attempt_id, request_digest FROM llm_output_repairs WHERE source_model_call_id = ?",
            vec![command.source_model_call_id.clone().into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "id")? != command.repair_id
        || row.try_get::<String>("", "node_instance_id")? != command.node_instance_id
        || row.try_get::<String>("", "originating_attempt_id")?
            != command.fence.invoking_node_attempt_id
        || row.try_get::<String>("", "request_digest")? != digest
    {
        return Err(StorageError::Conflict("llm_output_repair_replay"));
    }
    let checkpoint_row = connection
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("repair checkpoint is missing".into()))?;
    let checkpoint: zhuangsheng_core::llm::LlmLoopCheckpoint = load_object_json(
        connection,
        &checkpoint_row.try_get::<String>("", "checkpoint_object_id")?,
    )
    .await?;
    let transcript = load_transcript(connection, &checkpoint.transcript_ref).await?;
    if !checkpoint.checksum_is_valid()
        || checkpoint.effect_watermark != format!("outputrepair:{}", command.repair_id)
        || transcript.last() != Some(&command.instruction)
    {
        return Err(StorageError::Conflict("llm_output_repair_replay"));
    }
    Ok(Some(PreparedLlmOutputRepair {
        repair_id: command.repair_id.clone(),
        repair_no: u64::try_from(row.try_get::<i64>("", "repair_no")?)
            .map_err(|_| StorageError::Integrity("invalid repair number".into()))?,
        checkpoint,
        transcript,
        replayed: true,
    }))
}

async fn append_repair_event<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareLlmOutputRepairCommand,
    repair_no: u64,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT run_id FROM node_instances WHERE id = ?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("repair owner run is missing".into()))?;
    let run_id: String = row.try_get("", "run_id")?;
    append_event(connection, Event {
        run_id: &run_id,
        event_type: "llm.output.repair_prepared",
        importance: "info",
        node_instance_id: Some(&command.node_instance_id),
        attempt_id: Some(&command.fence.invoking_node_attempt_id),
        payload: json!({"schemaVersion":1,"repairId":command.repair_id,"repairNo":repair_no,"sourceModelCallId":command.source_model_call_id,"errorCode":command.error_code,"extractedBytesDigest":command.extracted_bytes_digest}),
        now,
    }).await?;
    Ok(())
}
