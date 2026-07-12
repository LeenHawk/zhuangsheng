use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{
    LlmLogicalCallStatus, LlmLoopCheckpoint, LlmOperationExecutionPin, RetryReadyResumeModelCall,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_bytes, sql},
};

pub(super) async fn load_retry_ready_model_call<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &LlmLoopCheckpoint,
) -> StorageResult<Option<RetryReadyResumeModelCall>> {
    let Some(active) = checkpoint.active_model_effect.as_ref() else {
        return Ok(None);
    };
    if active.status != LlmLogicalCallStatus::RetryReady {
        return Ok(None);
    }
    if !checkpoint.current_batch.is_empty() || active.response_ref.is_some() {
        return Err(StorageError::Integrity(
            "retry-ready model checkpoint has incompatible outputs".into(),
        ));
    }
    let row = connection
        .query_one_raw(sql(
            "SELECT mc.id AS model_call_id, mc.node_instance_id, mc.channel_id, mc.channel_revision_id, mc.model_id, mc.operation_key_json, mc.operation_taxonomy_version, mc.adapter_decoder_version, mc.request_object_id, mc.status AS model_status, e.id AS effect_id, e.status AS effect_status, e.classification FROM model_calls mc JOIN effects e ON e.model_call_id = mc.id WHERE mc.id = ?",
            vec![active.model_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "model_call",
            id: active.model_call_id.clone(),
        })?;
    if row.try_get::<String>("", "node_instance_id")? != checkpoint.node_instance_id
        || row.try_get::<String>("", "model_call_id")? != active.model_call_id
        || row.try_get::<String>("", "effect_id")? != active.effect_id
        || row.try_get::<String>("", "model_status")? != "retry_ready"
        || row.try_get::<String>("", "effect_status")? != "pending"
        || row.try_get::<String>("", "classification")? == "non_idempotent"
    {
        return Err(StorageError::Integrity(
            "retry-ready model call does not match its checkpoint".into(),
        ));
    }
    let taxonomy = u32::try_from(row.try_get::<i64>("", "operation_taxonomy_version")?)
        .map_err(|_| StorageError::Integrity("invalid operation taxonomy version".into()))?;
    let decoder = u32::try_from(row.try_get::<i64>("", "adapter_decoder_version")?)
        .map_err(|_| StorageError::Integrity("invalid adapter decoder version".into()))?;
    let operation_key = serde_json::from_str(&row.try_get::<String>("", "operation_key_json")?)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let request_bytes =
        load_object_bytes(connection, &row.try_get::<String>("", "request_object_id")?).await?;
    Ok(Some(RetryReadyResumeModelCall {
        model_call_id: active.model_call_id.clone(),
        effect_id: active.effect_id.clone(),
        channel_id: row.try_get("", "channel_id")?,
        operation: LlmOperationExecutionPin {
            channel_revision_id: row.try_get("", "channel_revision_id")?,
            model_id: row.try_get("", "model_id")?,
            operation_key,
            operation_taxonomy_version: taxonomy,
            adapter_decoder_version: decoder,
        },
        request_bytes,
    }))
}
