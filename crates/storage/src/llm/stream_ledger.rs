use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    canonical,
    llm::{PersistLlmStreamChunkCommand, PersistedLlmStreamChunk},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::sql,
    runtime::{Event, append_event},
};

use super::validation::validate_fence;

impl SqliteStore {
    pub async fn persist_llm_stream_chunk(
        &self,
        command: PersistLlmStreamChunkCommand,
        now: i64,
    ) -> StorageResult<PersistedLlmStreamChunk> {
        if command.node_instance_id.is_empty()
            || command.model_call_id.is_empty()
            || command.effect_attempt_id.is_empty()
            || command.chunk_no == 0
            || command.events.is_empty()
            || command.events.len() > 64
            || command
                .events
                .iter()
                .any(|event| event.call_id() != command.model_call_id || event.is_terminal())
        {
            return Err(StorageError::InvalidArgument(
                "LLM stream chunk is outside supported bounds".into(),
            ));
        }
        let payload = serde_json::json!({
            "schemaVersion":1,
            "modelCallId":command.model_call_id,
            "chunkNo":command.chunk_no,
            "events":command.events,
        });
        let payload_bytes = canonical::to_vec(&payload)?;
        if payload_bytes.len() > 256 * 1024 {
            return Err(StorageError::InvalidArgument(
                "LLM stream chunk exceeds 256 KiB".into(),
            ));
        }
        let payload_digest = canonical::hash_bytes(&payload_bytes);
        let transaction = self.db.begin().await?;
        let fenced =
            validate_fence(&transaction, &command.effect_attempt_id, &command.fence).await?;
        if fenced.node_instance_id != command.node_instance_id
            || fenced.model_call_id != command.model_call_id
            || fenced.attempt_status != "started"
            || fenced.effect_status != "pending"
            || fenced.model_status != "running"
        {
            return Err(StorageError::Conflict("llm_stream_owner_status"));
        }
        if let Some(row) = transaction
            .query_one_raw(sql(
                "SELECT model_call_id, durable_seq, payload_digest FROM llm_stream_chunks WHERE effect_attempt_id = ? AND chunk_no = ?",
                vec![
                    command.effect_attempt_id.clone().into(),
                    i64::try_from(command.chunk_no)
                        .map_err(|_| StorageError::InvalidArgument("stream chunk number is too large".into()))?
                        .into(),
                ],
            ))
            .await?
        {
            if row.try_get::<String>("", "model_call_id")? != command.model_call_id
                || row.try_get::<String>("", "payload_digest")? != payload_digest
            {
                return Err(StorageError::Conflict("llm_stream_chunk_replay"));
            }
            let durable_seq = u64::try_from(row.try_get::<i64>("", "durable_seq")?)
                .map_err(|_| StorageError::Integrity("invalid stream event sequence".into()))?;
            transaction.commit().await?;
            return Ok(PersistedLlmStreamChunk {
                durable_seq,
                replayed: true,
            });
        }
        let run = transaction
            .query_one_raw(sql(
                "SELECT run_id FROM node_instances WHERE id = ?",
                vec![command.node_instance_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::Integrity("stream owner run is missing".into()))?;
        let run_id: String = run.try_get("", "run_id")?;
        let durable_seq = append_event(
            &transaction,
            Event {
                run_id: &run_id,
                event_type: "llm.stream.chunk",
                importance: "info",
                node_instance_id: Some(&command.node_instance_id),
                attempt_id: Some(&command.fence.invoking_node_attempt_id),
                payload,
                now,
            },
        )
        .await?;
        transaction
            .execute_raw(sql(
                "INSERT INTO llm_stream_chunks (effect_attempt_id, chunk_no, model_call_id, run_id, durable_seq, payload_digest, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
                vec![
                    command.effect_attempt_id.into(),
                    i64::try_from(command.chunk_no)
                        .map_err(|_| StorageError::InvalidArgument("stream chunk number is too large".into()))?
                        .into(),
                    command.model_call_id.into(),
                    run_id.into(),
                    durable_seq.into(),
                    payload_digest.into(),
                    now.into(),
                ],
            ))
            .await?;
        transaction.commit().await?;
        Ok(PersistedLlmStreamChunk {
            durable_seq: u64::try_from(durable_seq)
                .map_err(|_| StorageError::Integrity("invalid stream event sequence".into()))?,
            replayed: false,
        })
    }
}
