use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{canonical, context_merge::MergeContextCommand};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, sql},
};

pub(super) async fn append_event<C: ConnectionTrait>(
    connection: &C,
    command: &MergeContextCommand,
    base_commit_id: &str,
    commit_id: &str,
    sequence: i64,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT OR IGNORE INTO domain_event_counters (aggregate_kind, aggregate_id, lineage_key, next_seq) VALUES ('working_context', ?, ?, 1)",
        vec![command.context_id.clone().into(), command.target_branch_id.clone().into()],
    )).await?;
    let row = connection.query_one_raw(sql(
        "SELECT next_seq FROM domain_event_counters WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ?",
        vec![command.context_id.clone().into(), command.target_branch_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::Integrity("merge event counter is missing".into()))?;
    let event_seq: i64 = row.try_get("", "next_seq")?;
    let updated = connection.execute_raw(sql(
        "UPDATE domain_event_counters SET next_seq = next_seq + 1 WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND next_seq = ?",
        vec![command.context_id.clone().into(), command.target_branch_id.clone().into(), event_seq.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("domain_event_sequence"));
    }
    connection.execute_raw(sql(
        "INSERT INTO domain_events (id, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json, created_at) VALUES (?, 'working_context', ?, ?, ?, 'context.merge.created', 1, ?, ?)",
        vec![new_id("domain_event").into(), command.context_id.clone().into(), command.target_branch_id.clone().into(), event_seq.into(), canonical::to_string(&json!({"schemaVersion":1,"commitId":commit_id,"sequenceNo":sequence,"baseCommitId":base_commit_id,"sourceHeadCommitId":command.expected_source_head}))?.into(), now.into()],
    )).await?;
    Ok(())
}
