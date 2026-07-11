use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    canonical,
    graph::EffectClassification,
    llm::{
        EffectAttemptStatus, EffectStatus, LlmLogicalCallStatus, LlmLoopCheckpoint,
        PrepareModelCallCommand, PreparedModelCall,
    },
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::{model_ledger::StoredOutcome, validation::FencedModelCall};

pub(super) fn validate_prepare_fields(command: &PrepareModelCallCommand) -> StorageResult<()> {
    if [
        &command.model_call_id,
        &command.effect_id,
        &command.effect_attempt_id,
        &command.node_instance_id,
        &command.originating_attempt_id,
        &command.channel_id,
        &command.effect_kind,
        &command.effect_operation_key,
        &command.effect_idempotency_key,
    ]
    .iter()
    .any(|value| value.is_empty() || value.len() > 256)
        || command.call_no == 0
        || command.request_bytes.is_empty()
        || command.request_bytes.len() > 16 * 1024 * 1024
        || command.retry_policy.max_attempts == 0
        || command.retry_policy.max_attempts > 32
        || command.retry_policy.backoff_ms.len() > 31
    {
        return Err(StorageError::InvalidArgument(
            "model call prepare command is outside supported bounds".into(),
        ));
    }
    Ok(())
}

pub(super) async fn load_existing<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareModelCallCommand,
    operation_json: &str,
    request_digest: &str,
    retry_json: &str,
) -> StorageResult<Option<PreparedModelCall>> {
    let row = connection
        .query_one(sql(
            "SELECT mc.id AS model_call_id, mc.originating_attempt_id, mc.channel_id, mc.channel_revision_id, mc.model_id, mc.operation_key_json, mc.operation_taxonomy_version, mc.adapter_decoder_version, mc.status AS model_status, co.content_hash AS request_digest, e.id AS effect_id, e.effect_kind, e.classification, e.operation_key, e.idempotency_key, e.retry_policy_json, e.status AS effect_status, ea.id AS effect_attempt_id, ea.invoking_node_attempt_id, ea.status AS attempt_status FROM model_calls mc JOIN content_objects co ON co.id = mc.request_object_id JOIN effects e ON e.model_call_id = mc.id JOIN effect_attempts ea ON ea.effect_id = e.id AND ea.attempt_no = 1 WHERE mc.node_instance_id = ? AND mc.call_no = ?",
            vec![command.node_instance_id.clone().into(), i64::try_from(command.call_no).map_err(|_| StorageError::InvalidArgument("model call number is too large".into()))?.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let taxonomy: i64 = row.try_get("", "operation_taxonomy_version")?;
    let decoder: i64 = row.try_get("", "adapter_decoder_version")?;
    let matches = row.try_get::<String>("", "model_call_id")? == command.model_call_id
        && row.try_get::<String>("", "effect_id")? == command.effect_id
        && row.try_get::<String>("", "effect_attempt_id")? == command.effect_attempt_id
        && row.try_get::<String>("", "originating_attempt_id")? == command.originating_attempt_id
        && row.try_get::<String>("", "invoking_node_attempt_id")? == command.originating_attempt_id
        && row.try_get::<String>("", "channel_id")? == command.channel_id
        && row.try_get::<String>("", "channel_revision_id")?
            == command.operation.channel_revision_id
        && row.try_get::<String>("", "model_id")? == command.operation.model_id
        && row.try_get::<String>("", "operation_key_json")? == operation_json
        && u32::try_from(taxonomy).ok() == Some(command.operation.operation_taxonomy_version)
        && u32::try_from(decoder).ok() == Some(command.operation.adapter_decoder_version)
        && row.try_get::<String>("", "request_digest")? == request_digest
        && row.try_get::<String>("", "effect_kind")? == command.effect_kind
        && row.try_get::<String>("", "classification")?
            == classification_name(command.effect_classification)
        && row.try_get::<String>("", "operation_key")? == command.effect_operation_key
        && row.try_get::<String>("", "idempotency_key")? == command.effect_idempotency_key
        && row.try_get::<String>("", "retry_policy_json")? == retry_json;
    if !matches {
        return Err(StorageError::Conflict("model_call_replay"));
    }
    Ok(Some(PreparedModelCall {
        model_call_id: command.model_call_id.clone(),
        effect_id: command.effect_id.clone(),
        effect_attempt_id: command.effect_attempt_id.clone(),
        model_status: parse_model_status(&row.try_get::<String>("", "model_status")?)?,
        effect_status: parse_effect_status(&row.try_get::<String>("", "effect_status")?)?,
        attempt_status: parse_attempt_status(&row.try_get::<String>("", "attempt_status")?)?,
        replayed: true,
    }))
}

pub(super) async fn persist_checkpoint<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &LlmLoopCheckpoint,
    now: i64,
) -> StorageResult<()> {
    ensure_live_object(connection, &checkpoint.context_snapshot_ref).await?;
    ensure_live_object(connection, &checkpoint.transcript_ref).await?;
    let bytes = canonical::to_vec(checkpoint)?;
    let object_id = put_inline_object(connection, &bytes, now).await?;
    let old = connection
        .query_one(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![checkpoint.node_instance_id.clone().into()],
        ))
        .await?
        .map(|row| row.try_get::<String>("", "checkpoint_object_id"))
        .transpose()?;
    connection
        .execute(sql(
            "INSERT INTO llm_loop_checkpoints (node_instance_id, schema_version, last_updated_by_attempt_id, checkpoint_object_id, checkpoint_digest, effect_watermark, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(node_instance_id) DO UPDATE SET schema_version = excluded.schema_version, last_updated_by_attempt_id = excluded.last_updated_by_attempt_id, checkpoint_object_id = excluded.checkpoint_object_id, checkpoint_digest = excluded.checkpoint_digest, effect_watermark = excluded.effect_watermark, updated_at = excluded.updated_at",
            vec![
                checkpoint.node_instance_id.clone().into(),
                i64::from(checkpoint.schema_version).into(),
                checkpoint.last_updated_by_attempt_id.clone().into(),
                object_id.clone().into(),
                checkpoint.checksum.clone().into(),
                checkpoint.effect_watermark.clone().into(),
                now.into(),
            ],
        ))
        .await?;
    if let Some(old) = old.filter(|old| old != &object_id) {
        connection
            .execute(sql(
                "DELETE FROM content_object_refs WHERE object_id = ? AND owner_kind = 'node_instance' AND owner_id = ? AND role = 'llm_checkpoint'",
                vec![old.into(), checkpoint.node_instance_id.clone().into()],
            ))
            .await?;
    }
    add_ref(
        connection,
        &object_id,
        "node_instance",
        &checkpoint.node_instance_id,
        "llm_checkpoint",
        now,
    )
    .await?;
    connection
        .execute(sql(
            "DELETE FROM content_object_refs WHERE owner_kind = 'node_instance' AND owner_id = ? AND role = 'llm_transcript' AND object_id <> ?",
            vec![checkpoint.node_instance_id.clone().into(), checkpoint.transcript_ref.clone().into()],
        ))
        .await?;
    add_ref(
        connection,
        &checkpoint.transcript_ref,
        "node_instance",
        &checkpoint.node_instance_id,
        "llm_transcript",
        now,
    )
    .await?;
    Ok(())
}

pub(super) async fn finish_rows<C: ConnectionTrait>(
    connection: &C,
    fenced: &FencedModelCall,
    effect_attempt_id: &str,
    stored: &StoredOutcome,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection
        .execute(sql(
            "UPDATE effect_attempts SET status = ?, result_object_id = ?, error_object_id = ?, finished_at = ? WHERE id = ? AND status = 'started'",
            vec![stored.attempt_status.into(), stored.result_object_id.clone().into(), stored.error_object_id.clone().into(), now.into(), effect_attempt_id.into()],
        ))
        .await?;
    let effect = connection
        .execute(sql(
            "UPDATE effects SET status = ?, result_object_id = ?, completed_at = ? WHERE id = ? AND status = 'pending'",
            vec![stored.effect_status.into(), stored.result_object_id.clone().into(), stored.effect_completed.then_some(now).into(), fenced.effect_id.clone().into()],
        ))
        .await?;
    let model = connection
        .execute(sql(
            "UPDATE model_calls SET status = ?, response_object_id = ?, usage_json = ?, finished_at = ? WHERE id = ? AND status = 'running'",
            vec![stored.model_status.into(), stored.result_object_id.clone().into(), stored.usage_json.clone().into(), now.into(), fenced.model_call_id.clone().into()],
        ))
        .await?;
    if attempt.rows_affected() != 1 || effect.rows_affected() != 1 || model.rows_affected() != 1 {
        return Err(StorageError::Conflict("model_effect_terminal_status"));
    }
    if let Some(object_id) = &stored.result_object_id {
        add_ref(
            connection,
            object_id,
            "effect",
            &fenced.effect_id,
            "result",
            now,
        )
        .await?;
        add_ref(
            connection,
            object_id,
            "effect_attempt",
            effect_attempt_id,
            "result",
            now,
        )
        .await?;
    }
    if let Some(object_id) = &stored.error_object_id {
        add_ref(
            connection,
            object_id,
            "effect_attempt",
            effect_attempt_id,
            "error",
            now,
        )
        .await?;
    }
    Ok(())
}

pub(super) fn require_states(
    fenced: &FencedModelCall,
    attempt: &str,
    effect: &str,
    model: &str,
) -> StorageResult<()> {
    if fenced.attempt_status == attempt
        && fenced.effect_status == effect
        && fenced.model_status == model
    {
        Ok(())
    } else {
        Err(StorageError::Conflict("model_effect_status"))
    }
}

pub(super) async fn add_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    owner_kind: &str,
    owner_id: &str,
    role: &str,
    now: i64,
) -> StorageResult<()> {
    connection
        .execute(sql(
            "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, ?, ?, ?, ?)",
            vec![object_id.into(), owner_kind.into(), owner_id.into(), role.into(), now.into()],
        ))
        .await?;
    Ok(())
}

async fn ensure_live_object<C: ConnectionTrait>(connection: &C, id: &str) -> StorageResult<()> {
    if connection
        .query_one(sql(
            "SELECT id FROM content_objects WHERE id = ? AND lifecycle = 'live'",
            vec![id.into()],
        ))
        .await?
        .is_none()
    {
        return Err(StorageError::InvalidArgument(
            "checkpoint references an unavailable content object".into(),
        ));
    }
    Ok(())
}

pub(super) fn classification_name(value: EffectClassification) -> &'static str {
    match value {
        EffectClassification::Pure => "pure",
        EffectClassification::Idempotent => "idempotent",
        EffectClassification::NonIdempotent => "non_idempotent",
    }
}

fn parse_model_status(value: &str) -> StorageResult<LlmLogicalCallStatus> {
    match value {
        "prepared" => Ok(LlmLogicalCallStatus::Prepared),
        "running" => Ok(LlmLogicalCallStatus::Running),
        "completed" => Ok(LlmLogicalCallStatus::Completed),
        "failed" => Ok(LlmLogicalCallStatus::Failed),
        "outcome_unknown" => Ok(LlmLogicalCallStatus::OutcomeUnknown),
        "retry_ready" => Ok(LlmLogicalCallStatus::RetryReady),
        "cancelled_before_start" => Ok(LlmLogicalCallStatus::CancelledBeforeStart),
        "abandoned_unknown" => Ok(LlmLogicalCallStatus::AbandonedUnknown),
        _ => Err(StorageError::Integrity("unknown model call status".into())),
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
        _ => Err(StorageError::Integrity("unknown effect status".into())),
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
            "unknown effect attempt status".into(),
        )),
    }
}
