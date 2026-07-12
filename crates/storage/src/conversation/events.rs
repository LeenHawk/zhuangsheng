use sea_orm::{ConnectionTrait, Statement};
use serde_json::Value;
use zhuangsheng_core::canonical;

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, sql},
};

pub(super) async fn append_event<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    event_type: &str,
    payload: &Value,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT OR IGNORE INTO domain_event_counters (aggregate_kind, aggregate_id, lineage_key, next_seq) VALUES ('conversation', ?, 'global', 1)",
        vec![conversation_id.into()],
    )).await?;
    let row = connection.query_one_raw(sql(
        "SELECT next_seq FROM domain_event_counters WHERE aggregate_kind = 'conversation' AND aggregate_id = ? AND lineage_key = 'global'",
        vec![conversation_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("conversation event counter is missing".into()))?;
    let sequence: i64 = row.try_get("", "next_seq")?;
    if sequence <= 0 {
        return Err(StorageError::Integrity(
            "conversation event counter is invalid".into(),
        ));
    }
    let updated = connection.execute_raw(Statement::from_sql_and_values(
        connection.get_database_backend(),
        "UPDATE domain_event_counters SET next_seq = ? WHERE aggregate_kind = 'conversation' AND aggregate_id = ? AND lineage_key = 'global' AND next_seq = ?",
        vec![(sequence + 1).into(), conversation_id.into(), sequence.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict(
            "conversation_event_sequence_conflict",
        ));
    }
    connection.execute_raw(sql(
        "INSERT INTO domain_events (id, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json, created_at) VALUES (?, 'conversation', ?, 'global', ?, ?, 1, ?, ?)",
        vec![new_id("domain_event").into(), conversation_id.into(), sequence.into(), event_type.into(), canonical::to_string(payload)?.into(), now.into()],
    )).await?;
    Ok(())
}
