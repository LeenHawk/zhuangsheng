use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::memory::MemorySearchCommand,
    canonical,
    llm::{
        ExecuteMemorySearchToolBatchCommand, MEMORY_SEARCH_BINDING_ID, MEMORY_SEARCH_TOOL_ID,
        MEMORY_SEARCH_TOOL_VERSION, MemorySearchToolBatchView, MemorySearchToolCallView,
        MemorySearchToolEnvelope,
    },
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_bytes, load_object_json, sql},
};

use super::tool_ledger_finish::validate_tool_output;

pub(super) async fn replay_memory_search_batch<C: ConnectionTrait>(
    connection: &C,
    command: &ExecuteMemorySearchToolBatchCommand,
) -> StorageResult<MemorySearchToolBatchView> {
    let rows = connection.query_all_raw(sql(
        "SELECT tc.id, tc.originating_attempt_id, tc.provider_call_id, tc.call_index, tc.binding_id, tc.tool_id, tc.tool_version, tc.call_digest, tc.arguments_object_id, tc.output_object_id, tc.status, br.envelope_object_id, br.result_digest, br.scope_snapshot_token, br.truncated, cp.checkpoint_digest FROM tool_calls tc JOIN tool_call_bound_read_results br ON br.tool_call_id = tc.id JOIN llm_loop_checkpoints cp ON cp.node_instance_id = tc.node_instance_id WHERE tc.model_call_id = ? ORDER BY tc.call_index",
        vec![command.model_call_id.clone().into()],
    )).await?;
    if rows.len() != command.calls.len() {
        return Err(StorageError::Conflict("memory_search_batch_replay"));
    }
    let mut checkpoint = command.checkpoint.clone();
    let mut views = Vec::with_capacity(rows.len());
    let mut stored_checkpoint_digest = None;
    for (row, call) in rows.iter().zip(&command.calls) {
        let query_ref: String = row.try_get("", "arguments_object_id")?;
        let envelope_ref: String = row.try_get("", "envelope_object_id")?;
        let output_ref: String = row.try_get("", "output_object_id")?;
        let query: MemorySearchCommand = load_object_json(connection, &query_ref).await?;
        let envelope: MemorySearchToolEnvelope =
            load_object_json(connection, &envelope_ref).await?;
        let result_digest = canonical::hash(&envelope)?;
        let matches = row.try_get::<String>("", "id")? == call.tool_call_id
            && row.try_get::<String>("", "originating_attempt_id")?
                == command.originating_attempt_id
            && row.try_get::<Option<String>>("", "provider_call_id")? == call.provider_call_id
            && u64::try_from(row.try_get::<i64>("", "call_index")?).ok() == Some(call.call_index)
            && row.try_get::<String>("", "binding_id")? == MEMORY_SEARCH_BINDING_ID
            && row.try_get::<String>("", "tool_id")? == MEMORY_SEARCH_TOOL_ID
            && row.try_get::<String>("", "tool_version")? == MEMORY_SEARCH_TOOL_VERSION
            && row.try_get::<String>("", "call_digest")? == call.call_digest
            && row.try_get::<String>("", "status")? == "completed"
            && query == call.query
            && row.try_get::<String>("", "result_digest")? == result_digest
            && row.try_get::<i64>("", "truncated")? == i64::from(envelope.truncated);
        if !matches {
            return Err(StorageError::Conflict("memory_search_batch_replay"));
        }
        validate_read_set(connection, &call.tool_call_id, &envelope).await?;
        validate_tool_output(&load_object_bytes(connection, &output_ref).await?)?;
        checkpoint
            .current_batch
            .iter_mut()
            .find(|checkpoint_call| checkpoint_call.tool_call_id == call.tool_call_id)
            .expect("validated memory search checkpoint")
            .output_ref = Some(output_ref.clone());
        let digest: String = row.try_get("", "checkpoint_digest")?;
        if stored_checkpoint_digest
            .replace(digest.clone())
            .is_some_and(|current| current != digest)
        {
            return Err(StorageError::Integrity(
                "memory search checkpoint projection diverged".into(),
            ));
        }
        views.push(MemorySearchToolCallView {
            tool_call_id: call.tool_call_id.clone(),
            call_index: call.call_index,
            query_ref,
            envelope_ref,
            output_ref,
            result_digest,
            scope_snapshot_token: row.try_get("", "scope_snapshot_token")?,
        });
    }
    checkpoint = checkpoint.seal()?;
    if stored_checkpoint_digest.as_deref() != Some(&checkpoint.checksum) {
        return Err(StorageError::Conflict("memory_search_checkpoint_replay"));
    }
    Ok(MemorySearchToolBatchView {
        calls: views,
        checkpoint,
        replayed: true,
    })
}

async fn validate_read_set<C: ConnectionTrait>(
    connection: &C,
    tool_call_id: &str,
    envelope: &MemorySearchToolEnvelope,
) -> StorageResult<()> {
    let rows = connection.query_all_raw(sql(
        "SELECT memory_id, commit_id, selection_ordinal, selected_content_hash FROM tool_call_read_set WHERE tool_call_id = ? ORDER BY selection_ordinal",
        vec![tool_call_id.into()],
    )).await?;
    if rows.len() != envelope.records.len() {
        return Err(StorageError::Integrity(
            "memory search read set size mismatch".into(),
        ));
    }
    for (ordinal, (row, record)) in rows.iter().zip(&envelope.records).enumerate() {
        if row.try_get::<String>("", "memory_id")? != record.memory_id
            || row.try_get::<String>("", "commit_id")? != record.commit_id
            || usize::try_from(row.try_get::<i64>("", "selection_ordinal")?).ok() != Some(ordinal)
            || row.try_get::<String>("", "selected_content_hash")? != record.content_hash
        {
            return Err(StorageError::Integrity(
                "memory search read set does not match its envelope".into(),
            ));
        }
    }
    Ok(())
}
