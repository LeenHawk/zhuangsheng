use sea_orm::QueryResult;
use serde::Deserialize;
use zhuangsheng_core::{canonical, conversation::CandidateProjectionErrorView};

use crate::{StorageError, StorageResult};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    schema_version: u32,
    code: String,
    safe_message: String,
}

pub(super) fn load_error(row: &QueryResult) -> StorageResult<Option<CandidateProjectionErrorView>> {
    let object_id: Option<String> = row.try_get("", "projection_error_object_id")?;
    let Some(_) = object_id else { return Ok(None) };
    let bytes: Option<Vec<u8>> = row.try_get("", "error_bytes")?;
    let bytes = bytes.ok_or_else(|| {
        StorageError::Integrity("candidate projection error object is unavailable".into())
    })?;
    if row
        .try_get::<Option<String>>("", "error_lifecycle")?
        .as_deref()
        != Some("live")
        || row.try_get::<Option<String>>("", "error_hash")?.as_deref()
            != Some(canonical::hash_bytes(&bytes).as_str())
        || row.try_get::<Option<i64>>("", "error_size")? != Some(bytes.len() as i64)
        || row.try_get::<i64>("", "error_ref")? != 1
    {
        return Err(StorageError::Integrity(
            "candidate projection error object is corrupt".into(),
        ));
    }
    let envelope: ErrorEnvelope = serde_json::from_slice(&bytes)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    if envelope.schema_version != 1 || envelope.code.is_empty() || envelope.safe_message.is_empty()
    {
        return Err(StorageError::Integrity(
            "candidate projection error payload is invalid".into(),
        ));
    }
    Ok(Some(CandidateProjectionErrorView {
        code: envelope.code,
        safe_message: envelope.safe_message,
    }))
}
