use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::graph::{GraphNode, RunLimits};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::events::{Event, append_event};

#[allow(clippy::too_many_arguments)]
pub(super) async fn check_limits<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    port: &str,
    key: &[u8],
    max_open_keys: u64,
    max_per_port: u64,
    limits: &RunLimits,
) -> StorageResult<Option<String>> {
    let counters = connection
        .query_one_raw(sql(
            "SELECT coordinator_buffered_values FROM run_execution_counters WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("run counters missing".into()))?;
    let total: i64 = counters.try_get("", "coordinator_buffered_values")?;
    let per_key = connection.query_one_raw(sql(
        "SELECT COUNT(*) AS count FROM coordination_buffer_items WHERE run_id = ? AND node_id = ? AND input_port = ? AND key_canonical = ? AND status = 'indexed'",
        vec![run_id.into(), node.id.clone().into(), port.into(), key.to_vec().into()],
    )).await?.ok_or_else(|| StorageError::Integrity("join count missing".into()))?;
    let per_key: i64 = per_key.try_get("", "count")?;
    let key_exists = connection.query_one_raw(sql(
        "SELECT 1 AS present FROM coordination_buffer_items WHERE run_id = ? AND node_id = ? AND key_canonical = ? AND status = 'indexed' LIMIT 1",
        vec![run_id.into(), node.id.clone().into(), key.to_vec().into()],
    )).await?.is_some();
    let open = connection.query_one_raw(sql(
        "SELECT COUNT(*) AS count FROM (SELECT key_canonical FROM coordination_buffer_items WHERE run_id = ? AND node_id = ? AND status = 'indexed' GROUP BY key_canonical)",
        vec![run_id.into(), node.id.clone().into()],
    )).await?.ok_or_else(|| StorageError::Integrity("join open-key count missing".into()))?;
    let open: i64 = open.try_get("", "count")?;
    if total.saturating_add(1) as u64 > limits.max_coordinator_buffered_values {
        Ok(Some("coordinator buffered value limit exceeded".into()))
    } else if per_key.saturating_add(1) as u64 > max_per_port {
        Ok(Some("join per-key per-port buffer limit exceeded".into()))
    } else if !key_exists && open.saturating_add(1) as u64 > max_open_keys {
        Ok(Some("join open-key limit exceeded".into()))
    } else {
        Ok(None)
    }
}

pub(super) async fn strand<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let rows = connection.query_all_raw(sql(
        "SELECT node_id, input_port, queue_value_id, enqueue_seq, key_json FROM coordination_buffer_items WHERE run_id = ? AND status = 'indexed' ORDER BY enqueue_seq",
        vec![run_id.into()],
    )).await?;
    for row in &rows {
        let key_json: String = row.try_get("", "key_json")?;
        append_event(connection, Event {
            run_id,
            event_type: "coordination.join_item_stranded",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1,"nodeId":row.try_get::<String>("", "node_id")?,"inputPort":row.try_get::<String>("", "input_port")?,"queueValueId":row.try_get::<String>("", "queue_value_id")?,"enqueueSeq":row.try_get::<i64>("", "enqueue_seq")?,"key":serde_json::from_str::<Value>(&key_json).map_err(|error| StorageError::Integrity(error.to_string()))?}),
            now,
        }).await?;
    }
    connection.execute_raw(sql(
        "UPDATE coordination_buffer_items SET status = 'stranded', terminal_at = ? WHERE run_id = ? AND status = 'indexed'",
        vec![now.into(), run_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET coordinator_buffered_values = coordinator_buffered_values - ? WHERE run_id = ?",
        vec![(rows.len() as i64).into(), run_id.into()],
    )).await?;
    Ok(())
}
