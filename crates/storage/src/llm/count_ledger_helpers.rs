use sea_orm::ConnectionTrait;
use serde_json::Value;
use zhuangsheng_core::{
    graph::EffectClassification,
    llm::{
        EffectAttemptStatus, EffectStatus, LlmLogicalCallStatus, PrepareCountCallCommand,
        PreparedCountCall,
    },
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
    runtime::{Event, append_event},
};

use super::model_ledger_helpers::{add_ref, classification_name};

pub(super) async fn load_existing<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareCountCallCommand,
    pin_digest: &str,
    candidate_digest: &str,
    request_digest: &str,
    retry_json: &str,
) -> StorageResult<Option<PreparedCountCall>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT cc.id AS count_call_id, cc.originating_attempt_id, cc.channel_id, cc.channel_revision_id, cc.model_id, cc.local_counter_id, cc.local_counter_version, cc.fallback_policy_version, cc.safety_margin_tokens, cc.count_execution_pin_digest, cc.trim_candidate_object_id, cc.trim_candidate_digest, cc.request_digest, cc.request_object_id, cc.status AS count_status, e.id AS effect_id, e.classification, e.idempotency_key, e.retry_policy_json, e.status AS effect_status, ea.id AS effect_attempt_id, ea.invoking_node_attempt_id, ea.status AS attempt_status, cp.checkpoint_object_id FROM count_calls cc JOIN effects e ON e.count_call_id = cc.id JOIN effect_attempts ea ON ea.effect_id = e.id AND ea.attempt_no = 1 JOIN llm_loop_checkpoints cp ON cp.node_instance_id = cc.node_instance_id WHERE cc.node_instance_id = ? AND cc.count_ordinal = ?",
            vec![
                command.node_instance_id.clone().into(),
                i64::try_from(command.count_ordinal)
                    .map_err(|_| StorageError::InvalidArgument("count ordinal is too large".into()))?
                    .into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let matches = row.try_get::<String>("", "count_call_id")? == command.count_call_id
        && row.try_get::<String>("", "effect_id")? == command.effect_id
        && row.try_get::<String>("", "effect_attempt_id")? == command.effect_attempt_id
        && row.try_get::<String>("", "originating_attempt_id")? == command.originating_attempt_id
        && row.try_get::<String>("", "invoking_node_attempt_id")? == command.originating_attempt_id
        && row.try_get::<String>("", "channel_id")? == command.channel_id
        && row.try_get::<String>("", "channel_revision_id")?
            == command.pin.generation_operation.channel_revision_id
        && row.try_get::<String>("", "model_id")? == command.pin.generation_operation.model_id
        && row.try_get::<String>("", "local_counter_id")? == command.pin.local_counter_id
        && u32::try_from(row.try_get::<i64>("", "local_counter_version")?).ok()
            == Some(command.pin.local_counter_version)
        && u32::try_from(row.try_get::<i64>("", "fallback_policy_version")?).ok()
            == Some(command.pin.fallback_policy_version)
        && u64::try_from(row.try_get::<i64>("", "safety_margin_tokens")?).ok()
            == Some(command.pin.safety_margin_tokens)
        && row.try_get::<String>("", "count_execution_pin_digest")? == pin_digest
        && row.try_get::<String>("", "trim_candidate_digest")? == candidate_digest
        && row.try_get::<String>("", "request_digest")? == request_digest
        && row.try_get::<String>("", "classification")?
            == classification_name(EffectClassification::Pure)
        && row.try_get::<String>("", "idempotency_key")? == command.effect_idempotency_key
        && row.try_get::<String>("", "retry_policy_json")? == retry_json;
    if !matches {
        return Err(StorageError::Conflict("count_call_replay"));
    }
    let checkpoint: zhuangsheng_core::llm::LlmLoopCheckpoint = load_object_json(
        connection,
        &row.try_get::<String>("", "checkpoint_object_id")?,
    )
    .await?;
    Ok(Some(PreparedCountCall {
        count_call_id: command.count_call_id.clone(),
        effect_id: command.effect_id.clone(),
        effect_attempt_id: command.effect_attempt_id.clone(),
        trim_candidate_ref: row.try_get("", "trim_candidate_object_id")?,
        request_ref: row.try_get("", "request_object_id")?,
        context_snapshot_ref: checkpoint.context_snapshot_ref,
        transcript_ref: checkpoint.transcript_ref,
        logical_status: parse_logical_status(&row.try_get::<String>("", "count_status")?)?,
        effect_status: parse_effect_status(&row.try_get::<String>("", "effect_status")?)?,
        attempt_status: parse_attempt_status(&row.try_get::<String>("", "attempt_status")?)?,
        replayed: true,
    }))
}

pub(super) async fn add_count_refs<C: ConnectionTrait>(
    connection: &C,
    count_call_id: &str,
    effect_attempt_id: &str,
    candidate_ref: &str,
    request_ref: &str,
    now: i64,
) -> StorageResult<()> {
    add_ref(
        connection,
        candidate_ref,
        "count_call",
        count_call_id,
        "trim_candidate",
        now,
    )
    .await?;
    add_ref(
        connection,
        request_ref,
        "count_call",
        count_call_id,
        "request",
        now,
    )
    .await?;
    add_ref(
        connection,
        request_ref,
        "effect_attempt",
        effect_attempt_id,
        "request",
        now,
    )
    .await
}

pub(super) async fn append_count_event<C: ConnectionTrait>(
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
        .ok_or_else(|| StorageError::Integrity("count event owner is unavailable".into()))?;
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

fn parse_logical_status(value: &str) -> StorageResult<LlmLogicalCallStatus> {
    match value {
        "prepared" => Ok(LlmLogicalCallStatus::Prepared),
        "running" => Ok(LlmLogicalCallStatus::Running),
        "completed" => Ok(LlmLogicalCallStatus::Completed),
        "failed" => Ok(LlmLogicalCallStatus::Failed),
        "retry_ready" => Ok(LlmLogicalCallStatus::RetryReady),
        "cancelled_before_start" => Ok(LlmLogicalCallStatus::CancelledBeforeStart),
        "abandoned_unknown" => Ok(LlmLogicalCallStatus::AbandonedUnknown),
        _ => Err(StorageError::Integrity("unknown count-call status".into())),
    }
}

fn parse_effect_status(value: &str) -> StorageResult<EffectStatus> {
    match value {
        "pending" => Ok(EffectStatus::Pending),
        "succeeded" => Ok(EffectStatus::Succeeded),
        "failed" => Ok(EffectStatus::Failed),
        "cancelled_before_start" => Ok(EffectStatus::CancelledBeforeStart),
        "abandoned_unknown" => Ok(EffectStatus::AbandonedUnknown),
        _ => Err(StorageError::Integrity(
            "unknown count effect status".into(),
        )),
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
            "unknown count effect attempt status".into(),
        )),
    }
}
