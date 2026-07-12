use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::Value;
use zhuangsheng_core::{
    application::tool::{ToolCallOutput, ToolOutputPart},
    canonical,
    llm::{
        LlmLogicalCallStatus, SettleToolBatchCommand, SettledToolBatch, ToolCallCheckpointStatus,
        ir::{LlmContentPartIr, LlmTurnItemIr, ToolResultOutcome, validate_transcript_ir},
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
};

use super::{
    model_ledger_helpers::persist_checkpoint,
    validation::{load_ledger_context, validate_node_attempt_fence},
};

impl SqliteStore {
    pub async fn settle_tool_batch(
        &self,
        command: SettleToolBatchCommand,
        now: i64,
    ) -> StorageResult<SettledToolBatch> {
        let transaction = self.db.begin().await?;
        validate_node_attempt_fence(&transaction, &command.node_instance_id, &command.fence)
            .await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        validate_batch_checkpoint(&transaction, &context, &command).await?;
        let mut transcript =
            load_transcript(&transaction, &command.checkpoint.transcript_ref).await?;
        let unresolved = unresolved_calls(&transcript);
        let rows = load_terminal_calls(&transaction, &command.model_call_id).await?;
        let checkpoint_matches = command.checkpoint.current_batch.len() == rows.len()
            && command
                .checkpoint
                .current_batch
                .iter()
                .zip(&rows)
                .all(|(checkpoint, row)| {
                    checkpoint.tool_call_id == row.id
                        && checkpoint.call_index == row.call_index
                        && checkpoint.status == checkpoint_status(&row.status)
                        && checkpoint.output_ref == row.output_ref
                });
        if unresolved.len() != rows.len() || rows.is_empty() || !checkpoint_matches {
            return Err(StorageError::InvalidArgument(
                "tool batch does not match unresolved transcript calls".into(),
            ));
        }
        for (ordinal, row) in rows.iter().enumerate() {
            let call = &unresolved[ordinal];
            let binding = context
                .snapshot
                .tools
                .iter()
                .find(|grant| grant.binding_id == row.binding_id)
                .ok_or_else(|| {
                    StorageError::Integrity("tool binding snapshot is missing".into())
                })?;
            let exposed_name = binding.exposed_name.as_deref().unwrap_or(&row.tool_id);
            if call.name != exposed_name || row.call_index != ordinal as u64 {
                return Err(StorageError::InvalidArgument(
                    "tool batch order or binding does not match transcript".into(),
                ));
            }
            let (outcome, content) = result_content(&transaction, &context.snapshot, row).await?;
            transcript.push(LlmTurnItemIr::ToolResult {
                id: format!("{}:result:{}", command.model_call_id, row.call_index),
                tool_call_id: call.id.clone(),
                tool_name: call.name.clone(),
                outcome,
                content,
            });
        }
        validate_transcript_ir(&transcript).map_err(|error| {
            StorageError::InvalidArgument(format!(
                "settled tool transcript is invalid: {}",
                error.message
            ))
        })?;
        let transcript_ref = put_inline_object(
            &transaction,
            &canonical::to_vec(&serde_json::json!({
                "schemaVersion":1,
                "items":transcript,
            }))?,
            now,
        )
        .await?;
        let mut checkpoint = command.checkpoint;
        checkpoint.transcript_ref = transcript_ref;
        checkpoint.last_updated_by_attempt_id = command.fence.invoking_node_attempt_id;
        checkpoint.effect_watermark = format!("toolbatch:{}", command.model_call_id);
        checkpoint = checkpoint.seal()?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        transaction.commit().await?;
        Ok(SettledToolBatch {
            checkpoint,
            transcript,
        })
    }
}

fn checkpoint_status(status: &str) -> ToolCallCheckpointStatus {
    match status {
        "completed" => ToolCallCheckpointStatus::Completed,
        "denied" => ToolCallCheckpointStatus::Denied,
        _ => ToolCallCheckpointStatus::Failed,
    }
}

struct TerminalToolCall {
    id: String,
    call_index: u64,
    binding_id: String,
    tool_id: String,
    status: String,
    output_ref: Option<String>,
    error_ref: Option<String>,
    output_size: Option<u64>,
}

async fn load_terminal_calls<C: ConnectionTrait>(
    connection: &C,
    model_call_id: &str,
) -> StorageResult<Vec<TerminalToolCall>> {
    connection
        .query_all_raw(sql(
            "SELECT tc.id, tc.call_index, tc.binding_id, tc.tool_id, tc.status, tc.output_object_id, tc.error_object_id, output.byte_size AS output_size FROM tool_calls tc LEFT JOIN content_objects output ON output.id = tc.output_object_id WHERE tc.model_call_id = ? ORDER BY tc.call_index",
            vec![model_call_id.into()],
        ))
        .await?
        .into_iter()
        .map(|row| {
            let status: String = row.try_get("", "status")?;
            if !matches!(status.as_str(), "completed" | "failed" | "denied") {
                return Err(StorageError::Conflict("tool_batch_not_terminal"));
            }
            Ok(TerminalToolCall {
                id: row.try_get("", "id")?,
                call_index: u64::try_from(row.try_get::<i64>("", "call_index")?)
                    .map_err(|_| StorageError::Integrity("invalid tool call index".into()))?,
                binding_id: row.try_get("", "binding_id")?,
                tool_id: row.try_get("", "tool_id")?,
                status,
                output_ref: row.try_get("", "output_object_id")?,
                error_ref: row.try_get("", "error_object_id")?,
                output_size: row
                    .try_get::<Option<i64>>("", "output_size")?
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|_| StorageError::Integrity("invalid tool output size".into()))?,
            })
        })
        .collect()
}

async fn result_content<C: ConnectionTrait>(
    connection: &C,
    snapshot: &zhuangsheng_core::graph::LlmNodeExecutionSnapshot,
    row: &TerminalToolCall,
) -> StorageResult<(ToolResultOutcome, Vec<LlmContentPartIr>)> {
    if row.status == "completed" {
        let descriptor = snapshot
            .tool_descriptors
            .iter()
            .find(|item| item.descriptor.tool_id == row.tool_id)
            .ok_or_else(|| StorageError::Integrity("tool descriptor snapshot is missing".into()))?;
        if row.output_size.is_none_or(|size| {
            size == 0 || size > descriptor.descriptor.limits.max_llm_result_bytes
        }) {
            return Err(StorageError::InvalidArgument(
                "tool output exceeds its pinned model-result limit".into(),
            ));
        }
        let output: ToolCallOutput = load_object_json(
            connection,
            row.output_ref.as_deref().ok_or_else(|| {
                StorageError::Integrity("completed tool output is missing".into())
            })?,
        )
        .await?;
        let mut llm_result = output.parts.into_iter().filter_map(|part| match part {
            ToolOutputPart::LlmResult { content } => Some(content),
            _ => None,
        });
        let content = llm_result
            .next()
            .filter(|content| !content.is_empty())
            .ok_or_else(|| StorageError::Integrity("tool LLM result is missing".into()))?;
        if llm_result.next().is_some() {
            return Err(StorageError::Integrity(
                "tool output has duplicate LLM results".into(),
            ));
        }
        return Ok((ToolResultOutcome::Success, content));
    }
    let error: Value = load_object_json(
        connection,
        row.error_ref
            .as_deref()
            .ok_or_else(|| StorageError::Integrity("terminal tool error is missing".into()))?,
    )
    .await?;
    let message = error
        .get("safeMessage")
        .or_else(|| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("tool call failed");
    let safe_message: String = message.chars().take(512).collect();
    Ok((
        if row.status == "denied" {
            ToolResultOutcome::Denied
        } else {
            ToolResultOutcome::Error
        },
        vec![LlmContentPartIr::Text { text: safe_message }],
    ))
}

fn unresolved_calls(transcript: &[LlmTurnItemIr]) -> Vec<zhuangsheng_core::llm::ir::ToolCallIr> {
    let resolved: BTreeSet<&str> = transcript
        .iter()
        .filter_map(|item| match item {
            LlmTurnItemIr::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect();
    transcript
        .iter()
        .filter_map(|item| match item {
            LlmTurnItemIr::AssistantToolCall { call, .. }
                if !resolved.contains(call.id.as_str()) =>
            {
                Some(call.clone())
            }
            _ => None,
        })
        .collect()
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

async fn validate_batch_checkpoint<C: ConnectionTrait>(
    connection: &C,
    context: &super::validation::LedgerContext,
    command: &SettleToolBatchCommand,
) -> StorageResult<()> {
    let stored = connection
        .query_one_raw(sql(
            "SELECT checkpoint_digest FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("LLM checkpoint is missing".into()))?;
    let active_model_matches =
        command
            .checkpoint
            .active_model_effect
            .as_ref()
            .is_some_and(|active| {
                active.model_call_id == command.model_call_id
                    && active.status == LlmLogicalCallStatus::Completed
            });
    let terminal = command.checkpoint.current_batch.iter().all(|call| {
        matches!(
            call.status,
            ToolCallCheckpointStatus::Completed
                | ToolCallCheckpointStatus::Failed
                | ToolCallCheckpointStatus::Denied
        )
    });
    if !command.checkpoint.checksum_is_valid()
        || command.checkpoint.node_instance_id != command.node_instance_id
        || command.checkpoint.graph_revision_id != context.graph_revision_id
        || command.checkpoint.registry_snapshot != context.snapshot.tool_registry
        || command.checkpoint.current_batch.is_empty()
        || !active_model_matches
        || !terminal
        || stored.try_get::<String>("", "checkpoint_digest")? != command.checkpoint.checksum
    {
        return Err(StorageError::InvalidArgument(
            "LLM checkpoint cannot settle the tool batch".into(),
        ));
    }
    Ok(())
}
