use std::collections::BTreeSet;

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    graph::MemoryToolCapability,
    llm::{
        ExecuteMemorySearchToolBatchCommand, LlmLogicalCallStatus,
        MemorySearchToolCallDigestMaterial, TOOL_CALL_POLICY_VERSION, ToolCallCheckpointStatus,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::validation::LedgerContext;

pub(super) struct MemorySearchBatchValidation {
    pub existing_model_calls: u64,
}

pub(super) async fn validate_memory_search_batch<C: ConnectionTrait>(
    connection: &C,
    context: &LedgerContext,
    command: &ExecuteMemorySearchToolBatchCommand,
) -> StorageResult<MemorySearchBatchValidation> {
    validate_fields(command)?;
    validate_model_owner(connection, command).await?;
    for call in &command.calls {
        let memory =
            context.snapshot.memory.as_ref().ok_or_else(|| {
                StorageError::InvalidArgument("memory tools are not pinned".into())
            })?;
        let grants: Vec<_> = memory
            .tools
            .iter()
            .filter(|grant| {
                grant.capability == MemoryToolCapability::SearchMemory
                    && grant
                        .scopes
                        .iter()
                        .any(|scope| scope == &call.query.scope_id)
            })
            .collect();
        if grants.len() != 1 {
            return Err(StorageError::InvalidArgument(
                "search_memory scope grant is missing or ambiguous".into(),
            ));
        }
        let grant = grants[0];
        if call.query.limit > grant.max_results.unwrap_or(20) {
            return Err(StorageError::InvalidArgument(
                "search_memory result limit exceeds its grant".into(),
            ));
        }
        let mut normalized_tags = call.query.tags.clone();
        normalized_tags.sort();
        normalized_tags.dedup();
        if normalized_tags != call.query.tags
            || (MemorySearchToolCallDigestMaterial {
                query: call.query.clone(),
                grant: grant.clone(),
                policy_version: TOOL_CALL_POLICY_VERSION,
            })
            .digest()?
                != call.call_digest
        {
            return Err(StorageError::InvalidArgument(
                "search_memory call does not match its pinned grant".into(),
            ));
        }
    }
    let existing_model_calls =
        count_calls(connection, "model_call_id", &command.model_call_id).await?;
    if existing_model_calls != 0 && existing_model_calls != command.calls.len() as u64 {
        return Err(StorageError::Conflict("memory_search_batch_partial"));
    }
    let existing_total =
        count_calls(connection, "node_instance_id", &command.node_instance_id).await?;
    let expected_used = if existing_model_calls == 0 {
        existing_total
            .checked_add(command.calls.len() as u64)
            .ok_or_else(|| StorageError::Integrity("tool-call count overflow".into()))?
    } else {
        existing_total
    };
    let limit = context
        .snapshot
        .limits
        .max_tool_calls
        .ok_or_else(|| StorageError::Integrity("tool-call limit is not pinned".into()))?;
    if expected_used > limit {
        return Err(StorageError::InvalidArgument(
            "tool-call limit exceeded".into(),
        ));
    }
    validate_checkpoint(context, command, expected_used)?;
    Ok(MemorySearchBatchValidation {
        existing_model_calls,
    })
}

fn validate_fields(command: &ExecuteMemorySearchToolBatchCommand) -> StorageResult<()> {
    if command.calls.is_empty()
        || command.calls.len() > 32
        || [
            &command.node_instance_id,
            &command.originating_attempt_id,
            &command.model_call_id,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256)
    {
        return Err(StorageError::InvalidArgument(
            "search_memory batch is outside supported bounds".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    for (ordinal, call) in command.calls.iter().enumerate() {
        if call.tool_call_id.is_empty()
            || call.tool_call_id.len() > 256
            || call.call_digest.is_empty()
            || call.call_digest.len() > 256
            || call
                .provider_call_id
                .as_ref()
                .is_some_and(|id| id.len() > 256)
            || call.call_index != ordinal as u64
            || !ids.insert(&call.tool_call_id)
        {
            return Err(StorageError::InvalidArgument(
                "search_memory calls must be a complete ordered model batch".into(),
            ));
        }
    }
    Ok(())
}

async fn validate_model_owner<C: ConnectionTrait>(
    connection: &C,
    command: &ExecuteMemorySearchToolBatchCommand,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT node_instance_id, status FROM model_calls WHERE id = ?",
            vec![command.model_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "model_call",
            id: command.model_call_id.clone(),
        })?;
    if row.try_get::<String>("", "node_instance_id")? != command.node_instance_id
        || row.try_get::<String>("", "status")? != "completed"
    {
        return Err(StorageError::InvalidArgument(
            "search_memory model owner is incompatible".into(),
        ));
    }
    Ok(())
}

fn validate_checkpoint(
    context: &LedgerContext,
    command: &ExecuteMemorySearchToolBatchCommand,
    expected_used: u64,
) -> StorageResult<()> {
    let checkpoint = &command.checkpoint;
    let active_model_matches = checkpoint
        .active_model_effect
        .as_ref()
        .is_some_and(|active| {
            active.model_call_id == command.model_call_id
                && active.status == LlmLogicalCallStatus::Completed
        });
    let calls_match = checkpoint.current_batch.len() == command.calls.len()
        && checkpoint
            .current_batch
            .iter()
            .zip(&command.calls)
            .all(|(stored, call)| {
                stored.tool_call_id == call.tool_call_id
                    && stored.call_index == call.call_index
                    && stored.call_digest == call.call_digest
                    && stored.status == ToolCallCheckpointStatus::Completed
                    && stored.effect_id.is_none()
                    && stored.output_ref.is_none()
                    && stored.wait_id.is_none()
            });
    let last_call_id = &command.calls.last().expect("nonempty batch").tool_call_id;
    if checkpoint.schema_version != 1
        || !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != command.node_instance_id
        || checkpoint.last_updated_by_attempt_id != command.originating_attempt_id
        || checkpoint.graph_revision_id != context.graph_revision_id
        || checkpoint.context_snapshot_ref != context.execution_snapshot_object_id
        || checkpoint.tool_calls_used != expected_used
        || checkpoint.effect_watermark != *last_call_id
        || !active_model_matches
        || !calls_match
    {
        return Err(StorageError::InvalidArgument(
            "LLM checkpoint is incompatible with search_memory batch".into(),
        ));
    }
    Ok(())
}

async fn count_calls<C: ConnectionTrait>(
    connection: &C,
    column: &str,
    value: &str,
) -> StorageResult<u64> {
    let statement = match column {
        "model_call_id" => "SELECT COUNT(*) AS count FROM tool_calls WHERE model_call_id = ?",
        "node_instance_id" => "SELECT COUNT(*) AS count FROM tool_calls WHERE node_instance_id = ?",
        _ => return Err(StorageError::Integrity("unknown tool count scope".into())),
    };
    let count: i64 = connection
        .query_one_raw(sql(statement, vec![value.into()]))
        .await?
        .expect("count query returns a row")
        .try_get("", "count")?;
    u64::try_from(count).map_err(|_| StorageError::Integrity("invalid tool-call count".into()))
}
