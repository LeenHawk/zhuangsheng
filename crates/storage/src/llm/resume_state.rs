use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::Value;
use zhuangsheng_core::llm::{
    LlmResumeState, LoadLlmResumeStateCommand, PreparedResumeToolCall, RetryReadyResumeToolCall,
    ToolCallCheckpointStatus, ir::LlmTurnItemIr,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_bytes, load_object_json, sql},
};

use super::output_repair_resume::load_output_repair_resume;
use super::resume_count::load_retry_ready_count_call;
use super::resume_model::load_retry_ready_model_call;
use super::validation::validate_node_attempt_fence;

impl SqliteStore {
    pub async fn load_llm_resume_state(
        &self,
        command: LoadLlmResumeStateCommand,
    ) -> StorageResult<Option<LlmResumeState>> {
        let transaction = self.db.begin().await?;
        validate_node_attempt_fence(&transaction, &command.node_instance_id, &command.fence)
            .await?;
        let row = transaction
            .query_one_raw(sql(
                "SELECT checkpoint_object_id, checkpoint_digest, last_updated_by_attempt_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
                vec![command.node_instance_id.clone().into()],
            ))
            .await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let checkpoint: zhuangsheng_core::llm::LlmLoopCheckpoint = load_object_json(
            &transaction,
            &row.try_get::<String>("", "checkpoint_object_id")?,
        )
        .await?;
        if !checkpoint.checksum_is_valid()
            || checkpoint.checksum != row.try_get::<String>("", "checkpoint_digest")?
            || checkpoint.node_instance_id != command.node_instance_id
            || checkpoint.last_updated_by_attempt_id
                != row.try_get::<String>("", "last_updated_by_attempt_id")?
        {
            return Err(StorageError::Integrity(
                "LLM resume checkpoint is incompatible with the active attempt".into(),
            ));
        }
        let transcript = load_transcript(&transaction, &checkpoint.transcript_ref).await?;
        let repair = load_output_repair_resume(&transaction, &checkpoint, &transcript).await?;
        let retry_ready_model_call = load_retry_ready_model_call(&transaction, &checkpoint).await?;
        let retry_ready_count_call = load_retry_ready_count_call(&transaction, &checkpoint).await?;
        let prepared_tool_calls = load_prepared_calls(
            &transaction,
            &command.fence.invoking_node_attempt_id,
            &checkpoint,
        )
        .await?;
        let retry_ready_tool_calls = load_retry_ready_calls(&transaction, &checkpoint).await?;
        transaction.commit().await?;
        Ok(Some(LlmResumeState {
            checkpoint,
            transcript,
            output_repairs_used: repair.used,
            pending_output_repair: repair.pending,
            retry_ready_model_call,
            retry_ready_count_call,
            prepared_tool_calls,
            retry_ready_tool_calls,
        }))
    }
}

async fn load_retry_ready_calls<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &zhuangsheng_core::llm::LlmLoopCheckpoint,
) -> StorageResult<Vec<RetryReadyResumeToolCall>> {
    let expected = checkpoint
        .current_batch
        .iter()
        .filter(|call| call.status == ToolCallCheckpointStatus::RetryReady)
        .count();
    if expected == 0 {
        return Ok(Vec::new());
    }
    let active_model_id = checkpoint
        .active_model_effect
        .as_ref()
        .map(|active| active.model_call_id.as_str())
        .ok_or_else(|| StorageError::Integrity("retry model call is missing".into()))?;
    let rows = connection
        .query_all_raw(sql(
            "SELECT tc.id AS tool_call_id, tc.model_call_id, tc.call_index, tc.binding_id, tc.tool_id, tc.tool_version, tc.arguments_object_id, tc.status AS tool_status, e.id AS effect_id, e.idempotency_key, e.status AS effect_status FROM tool_calls tc JOIN effects e ON e.tool_call_id = tc.id WHERE tc.model_call_id = ? AND tc.status = 'retry_ready' ORDER BY tc.call_index",
            vec![active_model_id.into()],
        ))
        .await?;
    if rows.len() != expected {
        return Err(StorageError::Integrity(
            "retry-ready tool calls do not match the checkpoint".into(),
        ));
    }
    let mut calls = Vec::with_capacity(expected);
    for row in rows {
        if row.try_get::<String>("", "effect_status")? != "pending" {
            return Err(StorageError::Conflict("retry_tool_status"));
        }
        let arguments_bytes = load_object_bytes(
            connection,
            &row.try_get::<String>("", "arguments_object_id")?,
        )
        .await?;
        let arguments: Value = serde_json::from_slice(&arguments_bytes)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
        calls.push(RetryReadyResumeToolCall {
            tool_call_id: row.try_get("", "tool_call_id")?,
            effect_id: row.try_get("", "effect_id")?,
            model_call_id: row.try_get("", "model_call_id")?,
            call_index: u64::try_from(row.try_get::<i64>("", "call_index")?)
                .map_err(|_| StorageError::Integrity("invalid tool call index".into()))?,
            binding_id: row.try_get("", "binding_id")?,
            tool_id: row.try_get("", "tool_id")?,
            tool_version: row.try_get("", "tool_version")?,
            arguments,
            effect_idempotency_key: row.try_get("", "idempotency_key")?,
        });
    }
    Ok(calls)
}

async fn load_prepared_calls<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    checkpoint: &zhuangsheng_core::llm::LlmLoopCheckpoint,
) -> StorageResult<Vec<PreparedResumeToolCall>> {
    let expected = checkpoint
        .current_batch
        .iter()
        .filter(|call| call.status == ToolCallCheckpointStatus::Prepared)
        .count();
    if expected == 0 {
        return Ok(Vec::new());
    }
    let active_model_id = checkpoint
        .active_model_effect
        .as_ref()
        .map(|active| active.model_call_id.as_str())
        .ok_or_else(|| StorageError::Integrity("resume model call is missing".into()))?;
    let rows = connection
        .query_all_raw(sql(
            "SELECT tc.id AS tool_call_id, tc.model_call_id, tc.call_index, tc.binding_id, tc.tool_id, tc.tool_version, tc.arguments_object_id, tc.status AS tool_status, e.id AS effect_id, e.idempotency_key, e.status AS effect_status, ea.id AS effect_attempt_id, ea.status AS attempt_status FROM tool_calls tc JOIN effects e ON e.tool_call_id = tc.id JOIN effect_attempts ea ON ea.effect_id = e.id AND ea.invoking_node_attempt_id = ? WHERE tc.model_call_id = ? AND tc.status = 'prepared' ORDER BY tc.call_index",
            vec![attempt_id.into(), active_model_id.into()],
        ))
        .await?;
    if rows.len() != expected {
        return Err(StorageError::Integrity(
            "resume tool effect attempts do not match the checkpoint".into(),
        ));
    }
    let mut calls = Vec::with_capacity(expected);
    for row in rows {
        if row.try_get::<String>("", "tool_status")? != "prepared"
            || row.try_get::<String>("", "effect_status")? != "pending"
            || row.try_get::<String>("", "attempt_status")? != "prepared"
        {
            return Err(StorageError::Conflict("resume_tool_status"));
        }
        let arguments_bytes = load_object_bytes(
            connection,
            &row.try_get::<String>("", "arguments_object_id")?,
        )
        .await?;
        let arguments: Value = serde_json::from_slice(&arguments_bytes)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
        calls.push(PreparedResumeToolCall {
            tool_call_id: row.try_get("", "tool_call_id")?,
            effect_id: row.try_get("", "effect_id")?,
            effect_attempt_id: row.try_get("", "effect_attempt_id")?,
            model_call_id: row.try_get("", "model_call_id")?,
            call_index: u64::try_from(row.try_get::<i64>("", "call_index")?)
                .map_err(|_| StorageError::Integrity("invalid tool call index".into()))?,
            binding_id: row.try_get("", "binding_id")?,
            tool_id: row.try_get("", "tool_id")?,
            tool_version: row.try_get("", "tool_version")?,
            arguments,
            effect_idempotency_key: row.try_get("", "idempotency_key")?,
        });
    }
    Ok(calls)
}

async fn load_transcript<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
) -> StorageResult<Vec<LlmTurnItemIr>> {
    let value: Value = load_object_json(connection, object_id).await?;
    serde_json::from_value(
        value
            .get("items")
            .cloned()
            .ok_or_else(|| StorageError::Integrity("LLM transcript items are missing".into()))?,
    )
    .map_err(|error| StorageError::Integrity(error.to_string()))
}
