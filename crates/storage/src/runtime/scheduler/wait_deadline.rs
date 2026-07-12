use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::canonical;

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
    runtime::{ResumeAttempt, create_resume_attempt},
};

use super::events::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run};

pub(super) async fn fire<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
    now: i64,
) -> StorageResult<()> {
    let row = connection.query_one_raw(sql(
        "SELECT w.run_id,w.node_instance_id,w.node_attempt_id,w.on_timeout,r.status AS run_status,r.control_epoch,ni.node_id FROM node_waits w JOIN graph_runs r ON r.id=w.run_id JOIN node_instances ni ON ni.id=w.node_instance_id WHERE w.id=? AND w.status='open'",
        vec![wait_id.into()],
    )).await?;
    let Some(row) = row else { return Ok(()) };
    let run_id: String = row.try_get("", "run_id")?;
    let instance_id: String = row.try_get("", "node_instance_id")?;
    let attempt_id: String = row.try_get("", "node_attempt_id")?;
    let policy: String = row.try_get("", "on_timeout")?;
    if policy == "resume_with_timeout" {
        return resume(
            connection,
            wait_id,
            &row,
            &run_id,
            &instance_id,
            &attempt_id,
            now,
        )
        .await;
    }
    if policy != "fail" {
        return Err(StorageError::Integrity(
            "unknown wait timeout policy".into(),
        ));
    }
    if connection
        .execute_raw(sql(
            "UPDATE node_waits SET status='expired',resolved_at=? WHERE id=? AND status='open'",
            vec![now.into(), wait_id.into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Ok(());
    }
    decrement_waits(connection, &run_id).await?;
    connection.execute_raw(sql("UPDATE node_attempts SET status='timed_out',finished_at=? WHERE id=? AND status='waiting'", vec![now.into(), attempt_id.clone().into()])).await?;
    connection.execute_raw(sql("UPDATE node_instances SET status='failed',updated_at=? WHERE id=? AND status='waiting'", vec![now.into(), instance_id.clone().into()])).await?;
    append_event(
        connection,
        Event {
            run_id: &run_id,
            event_type: "node.wait.expired",
            importance: "critical",
            node_instance_id: Some(&instance_id),
            attempt_id: Some(&attempt_id),
            payload: json!({"schemaVersion":1,"waitId":wait_id,"policy":"fail"}),
            now,
        },
    )
    .await?;
    fail_run(
        connection,
        &run_id,
        "wait_deadline_exceeded",
        "external wait deadline exceeded",
        now,
    )
    .await
}

async fn resume<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
    row: &sea_orm::QueryResult,
    run_id: &str,
    instance_id: &str,
    attempt_id: &str,
    now: i64,
) -> StorageResult<()> {
    let delivery_id = format!("timeout:{wait_id}");
    let response_ref = put_inline_object(
        connection,
        &canonical::to_vec(&json!({
            "schemaVersion":1,"kind":"value","value":{"timedOut":true}
        }))?,
        now,
    )
    .await?;
    if connection.execute_raw(sql(
        "UPDATE node_waits SET status='expired',response_object_id=?,accepted_delivery_id=?,resolved_at=? WHERE id=? AND status='open'",
        vec![response_ref.clone().into(), delivery_id.clone().into(), now.into(), wait_id.into()],
    )).await?.rows_affected() != 1 { return Ok(()) }
    decrement_waits(connection, run_id).await?;
    connection
        .execute_raw(sql(
            "UPDATE node_instances SET status='ready',updated_at=? WHERE id=? AND status='waiting'",
            vec![now.into(), instance_id.into()],
        ))
        .await?;
    add_object_ref(
        connection,
        &response_ref,
        "node_wait",
        wait_id,
        "response",
        now,
    )
    .await?;
    let control_epoch = u64::try_from(row.try_get::<i64>("", "control_epoch")?)
        .map_err(|_| StorageError::Integrity("invalid run control epoch".into()))?;
    let resume_id = create_resume_attempt(
        connection,
        ResumeAttempt {
            node_instance_id: instance_id,
            source_attempt_id: attempt_id,
            run_id,
            control_epoch,
            idempotency_key: &format!("wait:{delivery_id}:resume"),
        },
        now,
    )
    .await?;
    let run_status: String = row.try_get("", "run_status")?;
    if run_status == "waiting" {
        connection.execute_raw(sql("UPDATE graph_runs SET status='running',updated_at=? WHERE id=? AND status='waiting'", vec![now.into(), run_id.into()])).await?;
    }
    let seq = append_event(connection, Event { run_id, event_type: "node.wait.expired", importance: "critical", node_instance_id: Some(instance_id), attempt_id: Some(attempt_id), payload: json!({"schemaVersion":1,"waitId":wait_id,"policy":"resume_with_timeout","resumeAttemptId":resume_id}), now }).await?;
    if matches!(run_status.as_str(), "running" | "waiting") {
        let node_id: String = row.try_get("", "node_id")?;
        enqueue_wakeup(
            connection,
            run_id,
            Some(&node_id),
            "attempt_ready",
            seq,
            &format!("wait-timeout:{wait_id}"),
            now,
        )
        .await?;
    }
    Ok(())
}

async fn decrement_waits<C: ConnectionTrait>(connection: &C, run_id: &str) -> StorageResult<()> {
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET open_waits=open_waits-1 WHERE run_id=? AND open_waits>0",
        vec![run_id.into()],
    )).await?;
    Ok(())
}
