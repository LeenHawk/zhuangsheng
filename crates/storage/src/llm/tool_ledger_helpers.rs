use sea_orm::ConnectionTrait;
use serde_json::Value;
use zhuangsheng_core::llm::{
    EffectAttemptStatus, EffectStatus, PrepareToolCallCommand, PreparedToolCall,
    ToolCallCheckpointStatus,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::sql,
    runtime::{Event, append_event},
};

use super::model_ledger_helpers::{add_ref, classification_name};

pub(super) async fn load_existing<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolCallCommand,
    retry_json: &str,
) -> StorageResult<Option<PreparedToolCall>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT tc.id AS tool_call_id, tc.originating_attempt_id, tc.provider_call_id, tc.binding_id, tc.tool_id, tc.tool_version, tc.call_digest, tc.arguments_object_id, args.content_hash AS arguments_digest, tc.status AS tool_status, e.id AS effect_id, e.classification, e.operation_key, e.idempotency_key, e.retry_policy_json, e.status AS effect_status, ea.id AS effect_attempt_id, ea.invoking_node_attempt_id, ea.status AS attempt_status FROM tool_calls tc JOIN content_objects args ON args.id = tc.arguments_object_id JOIN effects e ON e.tool_call_id = tc.id JOIN effect_attempts ea ON ea.effect_id = e.id AND ea.attempt_no = 1 WHERE tc.model_call_id = ? AND tc.call_index = ?",
            vec![
                command.model_call_id.clone().into(),
                i64::try_from(command.call_index)
                    .map_err(|_| StorageError::InvalidArgument("tool call index is too large".into()))?
                    .into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let matches = row.try_get::<String>("", "tool_call_id")? == command.tool_call_id
        && row.try_get::<String>("", "effect_id")? == command.effect_id
        && row.try_get::<String>("", "effect_attempt_id")? == command.effect_attempt_id
        && row.try_get::<String>("", "originating_attempt_id")? == command.originating_attempt_id
        && row.try_get::<String>("", "invoking_node_attempt_id")? == command.originating_attempt_id
        && row.try_get::<Option<String>>("", "provider_call_id")? == command.provider_call_id
        && row.try_get::<String>("", "binding_id")? == command.binding_id
        && row.try_get::<String>("", "tool_id")? == command.tool_id
        && row.try_get::<String>("", "tool_version")? == command.tool_version
        && row.try_get::<String>("", "call_digest")? == command.call_digest
        && row.try_get::<String>("", "arguments_digest")?
            == zhuangsheng_core::canonical::hash_bytes(&command.arguments_bytes)
        && row.try_get::<String>("", "classification")?
            == classification_name(command.effect_classification)
        && row.try_get::<String>("", "operation_key")? == command.effect_operation_key
        && row.try_get::<String>("", "idempotency_key")? == command.effect_idempotency_key
        && row.try_get::<String>("", "retry_policy_json")? == retry_json;
    if !matches {
        return Err(StorageError::Conflict("tool_call_replay"));
    }
    Ok(Some(PreparedToolCall {
        tool_call_id: command.tool_call_id.clone(),
        effect_id: Some(command.effect_id.clone()),
        effect_attempt_id: Some(command.effect_attempt_id.clone()),
        arguments_ref: row.try_get("", "arguments_object_id")?,
        status: parse_tool_status(&row.try_get::<String>("", "tool_status")?)?,
        effect_status: Some(parse_effect_status(
            &row.try_get::<String>("", "effect_status")?,
        )?),
        attempt_status: Some(parse_attempt_status(
            &row.try_get::<String>("", "attempt_status")?,
        )?),
        replayed: true,
    }))
}

pub(super) async fn add_prepare_refs<C: ConnectionTrait>(
    connection: &C,
    tool_call_id: &str,
    effect_attempt_id: &str,
    arguments_ref: &str,
    now: i64,
) -> StorageResult<()> {
    add_ref(
        connection,
        arguments_ref,
        "tool_call",
        tool_call_id,
        "arguments",
        now,
    )
    .await?;
    add_ref(
        connection,
        arguments_ref,
        "effect_attempt",
        effect_attempt_id,
        "request",
        now,
    )
    .await
}

pub(super) async fn append_tool_event<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
    node_attempt_id: &str,
    event_type: &str,
    payload: Value,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT run_id FROM node_instances WHERE id = ?",
            vec![node_instance_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("tool event owner is unavailable".into()))?;
    let run_id: String = row.try_get("", "run_id")?;
    append_event(
        connection,
        Event {
            run_id: &run_id,
            event_type,
            importance: "info",
            node_instance_id: Some(node_instance_id),
            attempt_id: Some(node_attempt_id),
            payload,
            now,
        },
    )
    .await?;
    Ok(())
}

fn parse_tool_status(value: &str) -> StorageResult<ToolCallCheckpointStatus> {
    match value {
        "prepared" => Ok(ToolCallCheckpointStatus::Prepared),
        "running" => Ok(ToolCallCheckpointStatus::Running),
        "completed" => Ok(ToolCallCheckpointStatus::Completed),
        "failed" => Ok(ToolCallCheckpointStatus::Failed),
        "outcome_unknown" => Ok(ToolCallCheckpointStatus::OutcomeUnknown),
        "retry_ready" => Ok(ToolCallCheckpointStatus::RetryReady),
        "cancelled_before_start" => Ok(ToolCallCheckpointStatus::CancelledBeforeStart),
        "abandoned_unknown" => Ok(ToolCallCheckpointStatus::AbandonedUnknown),
        _ => Err(StorageError::Integrity(
            "unknown executable tool status".into(),
        )),
    }
}

fn parse_effect_status(value: &str) -> StorageResult<EffectStatus> {
    match value {
        "pending" => Ok(EffectStatus::Pending),
        "succeeded" => Ok(EffectStatus::Succeeded),
        "failed" => Ok(EffectStatus::Failed),
        "outcome_unknown" => Ok(EffectStatus::OutcomeUnknown),
        "cancelled_before_start" => Ok(EffectStatus::CancelledBeforeStart),
        "abandoned_unknown" => Ok(EffectStatus::AbandonedUnknown),
        _ => Err(StorageError::Integrity("unknown tool effect status".into())),
    }
}

fn parse_attempt_status(value: &str) -> StorageResult<EffectAttemptStatus> {
    match value {
        "prepared" => Ok(EffectAttemptStatus::Prepared),
        "started" => Ok(EffectAttemptStatus::Started),
        "succeeded" => Ok(EffectAttemptStatus::Succeeded),
        "failed" => Ok(EffectAttemptStatus::Failed),
        "outcome_unknown" => Ok(EffectAttemptStatus::OutcomeUnknown),
        "superseded_before_start" => Ok(EffectAttemptStatus::SupersededBeforeStart),
        _ => Err(StorageError::Integrity(
            "unknown tool effect attempt status".into(),
        )),
    }
}
