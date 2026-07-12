use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    runtime::{SubmitWaitResponseCommand, WaitDeliveryView},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    runtime::{Event, add_object_ref, append_event, enqueue_wakeup},
};

use super::wait_delivery::WaitContext;

pub(super) async fn persist_wait_response<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    decision_refs: &[String],
    prepared: &[String],
    denied: &[String],
    now: i64,
) -> StorageResult<String> {
    let response_ref=put_inline_object(connection,&canonical::to_vec(&json!({"schemaVersion":1,"kind":"tool_approval","decisionRefs":decision_refs,"preparedToolCallIds":prepared,"deniedToolCallIds":denied}))?,now).await?;
    if connection.execute_raw(sql("UPDATE node_waits SET status='resolved',response_object_id=?,accepted_delivery_id=?,resolved_at=? WHERE id=? AND status='open'",vec![response_ref.clone().into(),command.delivery_id.clone().into(),now.into(),command.wait_id.clone().into()])).await?.rows_affected()!=1{return Err(StorageError::Conflict("approval_wait_settle"));}
    add_object_ref(
        connection,
        &response_ref,
        "node_wait",
        &command.wait_id,
        "response",
        now,
    )
    .await?;
    Ok(response_ref)
}

pub(super) async fn persist_delivery<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    payload_digest: &str,
    response_ref: &str,
    view: &WaitDeliveryView,
    now: i64,
) -> StorageResult<()> {
    let result_ref = put_inline_object(connection, &canonical::to_vec(view)?, now).await?;
    connection.execute_raw(sql("INSERT INTO wait_deliveries (wait_id,delivery_id,payload_digest,result_object_id,created_at) VALUES (?,?,?,?,?)",vec![command.wait_id.clone().into(),command.delivery_id.clone().into(),payload_digest.into(),result_ref.clone().into(),now.into()])).await?;
    add_object_ref(
        connection,
        &result_ref,
        "node_wait",
        &command.wait_id,
        "delivery_result",
        now,
    )
    .await?;
    add_object_ref(
        connection,
        response_ref,
        "wait_delivery",
        &command.delivery_id,
        "response",
        now,
    )
    .await?;
    connection.execute_raw(sql("UPDATE run_execution_counters SET open_waits=open_waits-1 WHERE run_id=(SELECT run_id FROM node_waits WHERE id=?) AND open_waits>0",vec![command.wait_id.clone().into()])).await?;
    Ok(())
}

pub(super) async fn append_resolution_event<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    command: &SubmitWaitResponseCommand,
    resume_attempt_id: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    append_event_kind(
        connection,
        context,
        command,
        resume_attempt_id,
        "llm.tool.approval_resolved",
        "approval_run_status",
        now,
    )
    .await
}

pub(super) async fn append_memory_resolution_event<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    command: &SubmitWaitResponseCommand,
    resume_attempt_id: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    append_event_kind(
        connection,
        context,
        command,
        resume_attempt_id,
        "llm.tool.memory_proposals_resolved",
        "memory_proposal_run_status",
        now,
    )
    .await
}

async fn append_event_kind<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    command: &SubmitWaitResponseCommand,
    resume_attempt_id: Option<&str>,
    event_type: &str,
    conflict: &'static str,
    now: i64,
) -> StorageResult<()> {
    let runnable = matches!(context.run_status.as_str(), "running" | "waiting");
    if context.run_status=="waiting"&&connection.execute_raw(sql("UPDATE graph_runs SET status='running',updated_at=? WHERE id=? AND status='waiting'",vec![now.into(),context.run_id.clone().into()])).await?.rows_affected()!=1{return Err(StorageError::Conflict(conflict));}
    let seq=append_event(connection,Event{run_id:&context.run_id,event_type,importance:"critical",node_instance_id:Some(&context.node_instance_id),attempt_id:Some(&context.node_attempt_id),payload:json!({"schemaVersion":1,"waitId":command.wait_id,"deliveryId":command.delivery_id,"resumeAttemptId":resume_attempt_id}),now}).await?;
    if let Some(attempt_id) = resume_attempt_id.filter(|_| runnable) {
        enqueue_wakeup(
            connection,
            &context.run_id,
            Some(&context.node_id),
            "attempt_ready",
            seq,
            &format!("wait-resume:{attempt_id}"),
            now,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn replay_delivery<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    payload_digest: &str,
) -> StorageResult<Option<WaitDeliveryView>> {
    let row=connection.query_one_raw(sql("SELECT payload_digest,result_object_id FROM wait_deliveries WHERE wait_id=? AND delivery_id=?",vec![command.wait_id.clone().into(),command.delivery_id.clone().into()])).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "payload_digest")? != payload_digest {
        return Err(StorageError::IdempotencyConflict);
    }
    let mut view: WaitDeliveryView =
        load_object_json(connection, &row.try_get::<String>("", "result_object_id")?).await?;
    view.replayed = true;
    Ok(Some(view))
}

pub(super) fn validate_command(command: &SubmitWaitResponseCommand) -> StorageResult<()> {
    if [&command.wait_id, &command.delivery_id, &command.actor_kind]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256)
        || command
            .actor_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
        || !matches!(command.actor_kind.as_str(), "human" | "coordinator")
    {
        return Err(StorageError::InvalidArgument(
            "wait response is outside supported bounds".into(),
        ));
    }
    Ok(())
}
