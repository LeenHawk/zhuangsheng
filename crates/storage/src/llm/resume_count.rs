use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{
    CompletedResumeCountCall, CountResultSource, LlmLogicalCallStatus, LlmLoopCheckpoint,
    RetryReadyResumeCountCall,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_bytes, sql},
};

pub(super) async fn load_retry_ready_count_call<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &LlmLoopCheckpoint,
) -> StorageResult<Option<RetryReadyResumeCountCall>> {
    let Some(active) = checkpoint.active_count_effect.as_ref() else {
        return Ok(None);
    };
    if active.status != LlmLogicalCallStatus::RetryReady
        || checkpoint.count_calls_used <= checkpoint.model_calls_used
    {
        return Ok(None);
    }
    if active.result_ref.is_some() || active.result_source.is_some() {
        return Err(StorageError::Integrity(
            "retry-ready count checkpoint has a result".into(),
        ));
    }
    let row = connection
        .query_one_raw(sql(
            "SELECT cc.node_instance_id, cc.status AS count_status, cc.trim_candidate_object_id, cc.request_object_id, e.id AS effect_id, e.status AS effect_status, e.classification FROM count_calls cc JOIN effects e ON e.count_call_id = cc.id WHERE cc.id = ?",
            vec![active.count_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "count_call",
            id: active.count_call_id.clone(),
        })?;
    if row.try_get::<String>("", "node_instance_id")? != checkpoint.node_instance_id
        || row.try_get::<String>("", "effect_id")? != active.effect_id
        || row.try_get::<String>("", "count_status")? != "retry_ready"
        || row.try_get::<String>("", "effect_status")? != "pending"
        || row.try_get::<String>("", "classification")? != "pure"
    {
        return Err(StorageError::Integrity(
            "retry-ready count call does not match its checkpoint".into(),
        ));
    }
    let candidate_ref: String = row.try_get("", "trim_candidate_object_id")?;
    let request_ref: String = row.try_get("", "request_object_id")?;
    Ok(Some(RetryReadyResumeCountCall {
        count_call_id: active.count_call_id.clone(),
        effect_id: active.effect_id.clone(),
        trim_candidate_bytes: load_object_bytes(connection, &candidate_ref).await?,
        request_bytes: load_object_bytes(connection, &request_ref).await?,
    }))
}

pub(super) async fn load_completed_count_call<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &LlmLoopCheckpoint,
) -> StorageResult<Option<CompletedResumeCountCall>> {
    let Some(active) = checkpoint.active_count_effect.as_ref() else {
        return Ok(None);
    };
    if active.status != LlmLogicalCallStatus::Completed
        || checkpoint.count_calls_used <= checkpoint.model_calls_used
    {
        return Ok(None);
    }
    let result_ref = active.result_ref.as_ref().ok_or_else(|| {
        StorageError::Integrity("completed count checkpoint has no result".into())
    })?;
    let row = connection
        .query_one_raw(sql(
            "SELECT cc.node_instance_id, cc.status AS count_status, cc.result_source, cc.result_object_id, e.id AS effect_id, e.status AS effect_status FROM count_calls cc JOIN effects e ON e.count_call_id = cc.id WHERE cc.id = ?",
            vec![active.count_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "count_call",
            id: active.count_call_id.clone(),
        })?;
    let source = parse_source(&row.try_get::<String>("", "result_source")?)?;
    if row.try_get::<String>("", "node_instance_id")? != checkpoint.node_instance_id
        || row.try_get::<String>("", "effect_id")? != active.effect_id
        || row.try_get::<String>("", "count_status")? != "completed"
        || row.try_get::<String>("", "effect_status")? != "succeeded"
        || row.try_get::<String>("", "result_object_id")? != *result_ref
        || active.result_source != Some(source)
    {
        return Err(StorageError::Integrity(
            "completed count call does not match its checkpoint".into(),
        ));
    }
    let value: serde_json::Value =
        crate::graph::helpers::load_object_json(connection, result_ref).await?;
    let token_count = value
        .get("tokenCount")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| StorageError::Integrity("count result has no token count".into()))?;
    Ok(Some(CompletedResumeCountCall {
        token_count,
        source,
    }))
}

fn parse_source(value: &str) -> StorageResult<CountResultSource> {
    match value {
        "provider" => Ok(CountResultSource::Provider),
        "local" => Ok(CountResultSource::Local),
        "estimate" => Ok(CountResultSource::Estimate),
        _ => Err(StorageError::Integrity(
            "invalid count result source".into(),
        )),
    }
}
