use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::memory::{MemorySearchCommand, MemorySearchView},
    canonical,
    llm::{
        ExecuteMemorySearchToolBatchCommand, MEMORY_SEARCH_BINDING_ID, MEMORY_SEARCH_TOOL_ID,
        MEMORY_SEARCH_TOOL_VERSION, MemorySearchToolBatchView, MemorySearchToolCallView,
        MemorySearchToolEnvelope, MemorySearchToolRecord,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
    memory::search_in,
};

use super::{
    memory_search_tool_replay::replay_memory_search_batch,
    memory_search_tool_validation::validate_memory_search_batch,
    model_ledger_helpers::{add_ref, persist_checkpoint},
    tool_ledger_finish::validate_tool_output,
    tool_ledger_helpers::append_tool_event,
    validation::load_ledger_context,
};

struct ResolvedSearch {
    query: MemorySearchCommand,
    envelope: MemorySearchToolEnvelope,
    scope_snapshot_token: String,
}

impl SqliteStore {
    pub async fn execute_memory_search_tool_batch(
        &self,
        command: ExecuteMemorySearchToolBatchCommand,
        now: i64,
    ) -> StorageResult<MemorySearchToolBatchView> {
        let transaction = self.db.begin().await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        let validation = validate_memory_search_batch(&transaction, &context, &command).await?;
        if validation.existing_model_calls != 0 {
            let replay = replay_memory_search_batch(&transaction, &command).await?;
            transaction.commit().await?;
            return Ok(replay);
        }
        let mut resolved = Vec::with_capacity(command.calls.len());
        for call in &command.calls {
            let mut query = call.query.clone();
            let result = search_in(&transaction, &mut query).await?;
            resolved.push(resolve_search(&transaction, query, result).await?);
        }
        let mut checkpoint = command.checkpoint.clone();
        let mut views = Vec::with_capacity(command.calls.len());
        for (call, resolved) in command.calls.iter().zip(&resolved) {
            let query_ref =
                put_inline_object(&transaction, &canonical::to_vec(&resolved.query)?, now).await?;
            let envelope_bytes = canonical::to_vec(&resolved.envelope)?;
            let result_digest = canonical::hash(&resolved.envelope)?;
            let envelope_ref = put_inline_object(&transaction, &envelope_bytes, now).await?;
            let output = canonical::to_vec(&json!({
                "parts": [{
                    "type": "llm_result",
                    "content": [{
                        "type": "text",
                        "text": canonical::to_string(&resolved.envelope)?,
                    }],
                }],
            }))?;
            validate_tool_output(&output)?;
            let output_ref = put_inline_object(&transaction, &output, now).await?;
            persist_tool_call(
                &transaction,
                &command,
                call,
                &query_ref,
                &envelope_ref,
                &output_ref,
                &result_digest,
                resolved,
                now,
            )
            .await?;
            let checkpoint_call = checkpoint
                .current_batch
                .iter_mut()
                .find(|checkpoint_call| checkpoint_call.tool_call_id == call.tool_call_id)
                .expect("validated search_memory checkpoint");
            checkpoint_call.output_ref = Some(output_ref.clone());
            views.push(MemorySearchToolCallView {
                tool_call_id: call.tool_call_id.clone(),
                call_index: call.call_index,
                query_ref,
                envelope_ref,
                output_ref,
                result_digest,
                scope_snapshot_token: resolved.scope_snapshot_token.clone(),
            });
        }
        checkpoint = checkpoint.seal()?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        transaction.commit().await?;
        Ok(MemorySearchToolBatchView {
            calls: views,
            replayed: false,
        })
    }
}

async fn resolve_search<C: ConnectionTrait>(
    connection: &C,
    query: MemorySearchCommand,
    result: MemorySearchView,
) -> StorageResult<ResolvedSearch> {
    let mut records = Vec::with_capacity(result.records.len());
    for record in result.records {
        let commit_id = record.head_commit_id.ok_or_else(|| {
            StorageError::Integrity("selected memory record has no head commit".into())
        })?;
        let content_ref = record.content_ref.ok_or_else(|| {
            StorageError::Integrity("selected memory record has no content ref".into())
        })?;
        let content = record.content.ok_or_else(|| {
            StorageError::Integrity("selected memory record has no content".into())
        })?;
        let object = connection
            .query_one_raw(sql(
                "SELECT content_hash, lifecycle FROM content_objects WHERE id = ?",
                vec![content_ref.into()],
            ))
            .await?
            .ok_or_else(|| StorageError::Integrity("selected memory content is missing".into()))?;
        if object.try_get::<String>("", "lifecycle")? != "live" {
            return Err(StorageError::Integrity(
                "selected memory content is not live".into(),
            ));
        }
        records.push(MemorySearchToolRecord {
            memory_id: record.id,
            commit_id,
            content_hash: object.try_get("", "content_hash")?,
            summary: bounded_summary(&content.text),
            evidence_refs: Vec::new(),
        });
    }
    Ok(ResolvedSearch {
        query,
        envelope: MemorySearchToolEnvelope {
            records,
            truncated: result.truncated,
        },
        scope_snapshot_token: result.scope_snapshot_token,
    })
}

#[allow(clippy::too_many_arguments)]
async fn persist_tool_call<C: ConnectionTrait>(
    connection: &C,
    command: &ExecuteMemorySearchToolBatchCommand,
    call: &zhuangsheng_core::llm::MemorySearchToolCallCommand,
    query_ref: &str,
    envelope_ref: &str,
    output_ref: &str,
    result_digest: &str,
    resolved: &ResolvedSearch,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO tool_calls (id, node_instance_id, originating_attempt_id, model_call_id, provider_call_id, call_index, binding_id, tool_id, tool_version, call_digest, arguments_object_id, output_object_id, status, created_at, finished_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?)",
        vec![
            call.tool_call_id.clone().into(),
            command.node_instance_id.clone().into(),
            command.originating_attempt_id.clone().into(),
            command.model_call_id.clone().into(),
            call.provider_call_id.clone().into(),
            i64::try_from(call.call_index).map_err(|_| StorageError::InvalidArgument("tool call index is too large".into()))?.into(),
            MEMORY_SEARCH_BINDING_ID.into(),
            MEMORY_SEARCH_TOOL_ID.into(),
            MEMORY_SEARCH_TOOL_VERSION.into(),
            call.call_digest.clone().into(),
            query_ref.into(),
            output_ref.into(),
            now.into(),
            now.into(),
        ],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO tool_call_bound_read_results (tool_call_id, query_object_id, envelope_object_id, result_digest, scope_snapshot_token, truncated, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![
            call.tool_call_id.clone().into(),
            query_ref.into(),
            envelope_ref.into(),
            result_digest.into(),
            resolved.scope_snapshot_token.clone().into(),
            i64::from(resolved.envelope.truncated).into(),
            now.into(),
        ],
    )).await?;
    for (ordinal, record) in resolved.envelope.records.iter().enumerate() {
        connection.execute_raw(sql(
            "INSERT INTO tool_call_read_set (tool_call_id, memory_id, commit_id, selection_ordinal, selected_content_hash) VALUES (?, ?, ?, ?, ?)",
            vec![
                call.tool_call_id.clone().into(),
                record.memory_id.clone().into(),
                record.commit_id.clone().into(),
                i64::try_from(ordinal).map_err(|_| StorageError::Integrity("memory selection ordinal overflow".into()))?.into(),
                record.content_hash.clone().into(),
            ],
        )).await?;
    }
    for (object_id, role) in [
        (query_ref, "query"),
        (envelope_ref, "read_result"),
        (output_ref, "output"),
    ] {
        add_ref(
            connection,
            object_id,
            "tool_call",
            &call.tool_call_id,
            role,
            now,
        )
        .await?;
    }
    append_tool_event(
        connection,
        &command.node_instance_id,
        &command.originating_attempt_id,
        "llm.tool.memory_search_completed",
        json!({
            "schemaVersion": 1,
            "toolCallId": call.tool_call_id,
            "modelCallId": command.model_call_id,
            "callIndex": call.call_index,
            "resultDigest": result_digest,
            "scopeSnapshotToken": resolved.scope_snapshot_token,
            "resultCount": resolved.envelope.records.len(),
            "truncated": resolved.envelope.truncated,
        }),
        now,
    )
    .await
}

fn bounded_summary(text: &str) -> String {
    text.chars().take(2048).collect()
}
