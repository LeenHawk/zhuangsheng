use sea_orm::ConnectionTrait;
use serde_json::Value;
use zhuangsheng_core::canonical;

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, sql},
    llm::fence_run_effects,
};

pub(crate) struct Event<'a> {
    pub run_id: &'a str,
    pub event_type: &'a str,
    pub importance: &'a str,
    pub node_instance_id: Option<&'a str>,
    pub attempt_id: Option<&'a str>,
    pub payload: Value,
    pub now: i64,
}

pub(crate) async fn append_event<C: ConnectionTrait>(
    connection: &C,
    event: Event<'_>,
) -> StorageResult<i64> {
    let row = connection
        .query_one(sql(
            "SELECT next_seq FROM run_event_counters WHERE run_id = ?",
            vec![event.run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("run event counter missing".into()))?;
    let seq: i64 = row.try_get("", "next_seq")?;
    let updated = connection
        .execute(sql(
            "UPDATE run_event_counters SET next_seq = next_seq + 1 WHERE run_id = ? AND next_seq = ?",
            vec![event.run_id.into(), seq.into()],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_event_sequence"));
    }
    connection.execute(sql(
        "INSERT INTO run_events (id, run_id, seq, node_instance_id, attempt_id, event_type, schema_version, importance, payload_json, created_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
        vec![new_id("event").into(), event.run_id.into(), seq.into(), event.node_instance_id.map(String::from).into(), event.attempt_id.map(String::from).into(), event.event_type.into(), event.importance.into(), canonical::to_string(&event.payload)?.into(), event.now.into()],
    )).await?;
    Ok(seq)
}

pub(super) async fn finish_wakeup<C: ConnectionTrait>(
    connection: &C,
    wakeup_id: &str,
) -> StorageResult<()> {
    connection
        .execute(sql(
            "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE id = ? AND status IN ('claimed','pending')",
            vec![wakeup_id.into()],
        ))
        .await?;
    Ok(())
}

pub(crate) async fn add_object_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    owner_kind: &str,
    owner_id: &str,
    role: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute(sql(
        "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![object_id.into(), owner_kind.into(), owner_id.into(), role.into(), now.into()],
    )).await?;
    Ok(())
}

pub(crate) async fn enqueue_wakeup<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node_id: Option<&str>,
    kind: &str,
    caused_by_seq: i64,
    dedupe_key: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute(sql(
        "INSERT OR IGNORE INTO scheduler_wakeups (id, run_id, node_id, kind, caused_by_seq, dedupe_key, status, available_at, created_at) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
        vec![new_id("wakeup").into(), run_id.into(), node_id.map(String::from).into(), kind.into(), caused_by_seq.into(), dedupe_key.into(), now.into(), now.into()],
    )).await?;
    Ok(())
}

pub(crate) async fn fail_run<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    code: &str,
    safe_message: &str,
    now: i64,
) -> StorageResult<()> {
    let error = canonical::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "code": code,
        "category": "contract",
        "safeMessage": safe_message,
        "retryClass": "never"
    }))?;
    let error_id = crate::graph::helpers::put_inline_object(connection, &error, now).await?;
    let updated = connection.execute(sql(
        "UPDATE graph_runs SET status = 'failed', control_epoch = control_epoch + 1, drain_epoch = NULL, terminal_error_object_id = ?, finished_at = ?, updated_at = ? WHERE id = ? AND status IN ('created','running','waiting','interrupting','interrupted')",
        vec![error_id.clone().into(), now.into(), now.into(), run_id.into()],
    )).await?;
    if updated.rows_affected() == 0 {
        return Ok(());
    }
    let epoch_row = connection
        .query_one(sql(
            "SELECT control_epoch FROM graph_runs WHERE id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("failed run disappeared".into()))?;
    let terminal_epoch = u64::try_from(epoch_row.try_get::<i64>("", "control_epoch")?)
        .map_err(|_| StorageError::Integrity("invalid terminal run epoch".into()))?;
    fence_run_effects(connection, run_id, terminal_epoch, now).await?;
    connection.execute(sql(
        "UPDATE node_attempts SET status = 'cancelled', worker_id = NULL, lease_until = NULL, finished_at = ? WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('queued','leased','running','waiting')",
        vec![now.into(), run_id.into()],
    )).await?;
    connection.execute(sql(
        "UPDATE node_instances SET status = 'cancelled', updated_at = ? WHERE run_id = ? AND status IN ('ready','running','waiting')",
        vec![now.into(), run_id.into()],
    )).await?;
    connection.execute(sql(
        "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE run_id = ? AND status IN ('pending','claimed')",
        vec![run_id.into()],
    )).await?;
    connection.execute(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE run_id = ? AND status IN ('pending','ready')",
        vec![run_id.into()],
    )).await?;
    connection.execute(sql(
        "UPDATE node_waits SET status = 'cancelled', resolved_at = ? WHERE run_id = ? AND status = 'open'",
        vec![now.into(), run_id.into()],
    )).await?;
    connection
        .execute(sql(
            "UPDATE run_execution_counters SET open_waits = 0 WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await?;
    add_object_ref(
        connection,
        &error_id,
        "graph_run",
        run_id,
        "terminal_error",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id,
            event_type: "run.failed",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: serde_json::json!({"schemaVersion":1,"code":code,"safeMessage":safe_message}),
            now,
        },
    )
    .await?;
    Ok(())
}
