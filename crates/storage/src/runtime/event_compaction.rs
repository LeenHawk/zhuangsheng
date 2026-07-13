use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::Value;

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

use super::checkpoint_rows::{load_at_seq, validate_projection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEventCompactionReport {
    pub checkpoint_id: String,
    pub through_seq: u64,
    pub scanned: u64,
    pub compacted: u64,
}

impl SqliteStore {
    pub async fn compact_run_events(
        &self,
        run_id: &str,
        through_seq: u64,
        now: i64,
        limit: u32,
    ) -> StorageResult<RunEventCompactionReport> {
        if run_id.is_empty() || through_seq == 0 || limit == 0 || limit > 10_000 {
            return Err(StorageError::InvalidArgument(
                "invalid run event compaction request".into(),
            ));
        }
        let transaction = self.db.begin().await?;
        let checkpoint = load_at_seq(&transaction, run_id, through_seq)
            .await?
            .ok_or_else(|| StorageError::Conflict("runtime_checkpoint_required"))?;
        let snapshot: Value = load_object_json(&transaction, &checkpoint.snapshot_ref).await?;
        if checkpoint.schema_version != 1
            || zhuangsheng_core::canonical::hash(&snapshot)? != checkpoint.checksum
            || snapshot.get("throughSeq").and_then(Value::as_u64) != Some(through_seq)
            || snapshot.pointer("/run/id").and_then(Value::as_str) != Some(run_id)
        {
            return Err(StorageError::Integrity(
                "runtime checkpoint is invalid for event compaction".into(),
            ));
        }
        let current_through = current_through(&transaction, run_id).await?;
        validate_projection(&transaction, run_id, current_through).await?;
        let rows = transaction.query_all_raw(sql(
            "SELECT e.id,e.seq,e.event_type,e.schema_version,e.importance,e.payload_json,c.content_hash FROM run_events e LEFT JOIN content_objects c ON c.id=e.payload_object_id WHERE e.run_id=? AND e.seq<=? AND (e.importance='debug' OR e.event_type='llm.stream.chunk') ORDER BY e.seq LIMIT ?",
            vec![run_id.into(), i64::try_from(through_seq).map_err(|_| StorageError::InvalidArgument("event compaction sequence is too large".into()))?.into(), i64::from(limit).into()],
        )).await?;
        let scanned = rows.len() as u64;
        for row in &rows {
            let event_id: String = row.try_get("", "id")?;
            let seq: i64 = row.try_get("", "seq")?;
            let payload_hash = match row.try_get::<Option<String>>("", "payload_json")? {
                Some(payload) => {
                    let value: Value = serde_json::from_str(&payload)
                        .map_err(|_| StorageError::Integrity("event payload is invalid".into()))?;
                    zhuangsheng_core::canonical::hash(&value)?
                }
                None => row
                    .try_get::<Option<String>>("", "content_hash")?
                    .ok_or_else(|| {
                        StorageError::Integrity("event payload object is missing".into())
                    })?,
            };
            transaction.execute_raw(sql(
                "INSERT INTO run_event_compactions (run_id,seq,event_id,event_type,schema_version,importance,payload_hash,checkpoint_id,compacted_at) VALUES (?,?,?,?,?,?,?,?,?)",
                vec![run_id.into(),seq.into(),event_id.clone().into(),row.try_get::<String>("", "event_type")?.into(),row.try_get::<i64>("", "schema_version")?.into(),row.try_get::<String>("", "importance")?.into(),payload_hash.into(),checkpoint.id.clone().into(),now.into()],
            )).await?;
            let deleted = transaction.execute_raw(sql(
                "DELETE FROM run_events WHERE run_id=? AND seq=? AND id=? AND (importance='debug' OR event_type='llm.stream.chunk')",
                vec![run_id.into(),seq.into(),event_id.into()],
            )).await?;
            if deleted.rows_affected() != 1 {
                return Err(StorageError::Conflict("run_event_compaction_race"));
            }
        }
        transaction.commit().await?;
        Ok(RunEventCompactionReport {
            checkpoint_id: checkpoint.id,
            through_seq,
            scanned,
            compacted: scanned,
        })
    }
}

async fn current_through<C: ConnectionTrait>(connection: &C, run_id: &str) -> StorageResult<u64> {
    let row = connection
        .query_one_raw(sql(
            "SELECT next_seq - 1 AS through_seq FROM run_event_counters WHERE run_id=?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "run",
            id: run_id.into(),
        })?;
    u64::try_from(row.try_get::<i64>("", "through_seq")?)
        .map_err(|_| StorageError::Integrity("invalid runtime event sequence".into()))
}
