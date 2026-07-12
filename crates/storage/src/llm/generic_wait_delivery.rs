use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    runtime::{
        SubmitWaitResponseCommand, WaitDeliveryStatus, WaitDeliveryView, WaitResponsePayload,
    },
    schema::{self, JsonSchemaSpec},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    runtime::{
        Event, ResumeAttempt, add_object_ref, append_event, create_resume_attempt, enqueue_wakeup,
    },
};

use super::wait_delivery::WaitContext;

pub(super) struct GenericSettlement {
    pub response_ref: String,
    pub view: WaitDeliveryView,
}

pub(super) async fn settle_generic_response<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    context: &WaitContext,
    now: i64,
) -> StorageResult<GenericSettlement> {
    if !matches!(
        context.kind.as_str(),
        "human_response" | "webhook" | "external_job"
    ) || context.instance_status != "waiting"
    {
        return Err(StorageError::Conflict("wait_response_kind"));
    }
    if connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM wait_blockers WHERE wait_id=? LIMIT 1",
            vec![command.wait_id.clone().into()],
        ))
        .await?
        .is_some()
    {
        return Err(StorageError::Conflict("wait_response_kind"));
    }
    let value = match &command.payload {
        WaitResponsePayload::Value { value } => value,
        _ => return Err(StorageError::Conflict("wait_response_kind")),
    };
    validate_value(connection, &command.wait_id, value).await?;
    let response_ref = put_inline_object(
        connection,
        &canonical::to_vec(&json!({"schemaVersion":1,"kind":"value","value":value}))?,
        now,
    )
    .await?;
    if connection.execute_raw(sql(
        "UPDATE node_waits SET status='resolved',response_object_id=?,accepted_delivery_id=?,resolved_at=? WHERE id=? AND status='open'",
        vec![response_ref.clone().into(), command.delivery_id.clone().into(), now.into(), command.wait_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("wait_already_resolved"));
    }
    if connection
        .execute_raw(sql(
            "UPDATE node_instances SET status='ready',updated_at=? WHERE id=? AND status='waiting'",
            vec![now.into(), context.node_instance_id.clone().into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Err(StorageError::Conflict("wait_owner_status"));
    }
    add_object_ref(
        connection,
        &response_ref,
        "node_wait",
        &command.wait_id,
        "response",
        now,
    )
    .await?;
    let attempt_id = create_resume_attempt(
        connection,
        ResumeAttempt {
            node_instance_id: &context.node_instance_id,
            source_attempt_id: &context.node_attempt_id,
            run_id: &context.run_id,
            control_epoch: context.control_epoch,
            idempotency_key: &format!("wait:{}:resume", command.delivery_id),
        },
        now,
    )
    .await?;
    append_resolution(connection, command, context, &attempt_id, now).await?;
    Ok(GenericSettlement {
        response_ref,
        view: WaitDeliveryView {
            wait_id: command.wait_id.clone(),
            delivery_id: command.delivery_id.clone(),
            status: WaitDeliveryStatus::Resolved,
            prepared_tool_call_ids: Vec::new(),
            denied_tool_call_ids: Vec::new(),
            decided_memory_proposal_ids: Vec::new(),
            replayed: false,
        },
    })
}

async fn validate_value<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
    value: &serde_json::Value,
) -> StorageResult<()> {
    let row = connection.query_one_raw(sql(
        "SELECT response_schema_object_id,response_schema_compilation_object_id FROM node_waits WHERE id=?",
        vec![wait_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "wait", id: wait_id.into() })?;
    let schema_ref: Option<String> = row.try_get("", "response_schema_object_id")?;
    let compilation_ref: Option<String> =
        row.try_get("", "response_schema_compilation_object_id")?;
    match (schema_ref, compilation_ref) {
        (None, None) => Ok(()),
        (Some(schema_ref), Some(compilation_ref)) => {
            let spec: JsonSchemaSpec = load_object_json(connection, &schema_ref).await?;
            let stored: schema::SchemaCompilationDraft =
                load_object_json(connection, &compilation_ref).await?;
            if schema::compile(&spec)? != stored {
                return Err(StorageError::Integrity(
                    "wait response schema compilation changed".into(),
                ));
            }
            schema::validate(&spec, value).map_err(Into::into)
        }
        _ => Err(StorageError::Integrity(
            "incomplete wait response schema binding".into(),
        )),
    }
}

async fn append_resolution<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    context: &WaitContext,
    attempt_id: &str,
    now: i64,
) -> StorageResult<()> {
    let runnable = matches!(context.run_status.as_str(), "running" | "waiting");
    if context.run_status == "waiting" && connection.execute_raw(sql(
        "UPDATE graph_runs SET status='running',updated_at=? WHERE id=? AND status='waiting'",
        vec![now.into(), context.run_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("wait_run_status"));
    }
    let seq = append_event(connection, Event {
        run_id: &context.run_id,
        event_type: "node.wait.resolved",
        importance: "critical",
        node_instance_id: Some(&context.node_instance_id),
        attempt_id: Some(&context.node_attempt_id),
        payload: json!({"schemaVersion":1,"waitId":command.wait_id,"deliveryId":command.delivery_id,"resumeAttemptId":attempt_id}),
        now,
    }).await?;
    if runnable {
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
