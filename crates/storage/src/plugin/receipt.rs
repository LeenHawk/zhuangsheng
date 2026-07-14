use sea_orm::ConnectionTrait;
use serde::Serialize;

use crate::{StorageError, StorageResult, graph::helpers::*};

pub(super) fn require_key(key: &str) -> StorageResult<()> {
    if key.trim().is_empty() || key.len() > 256 {
        return Err(StorageError::InvalidArgument(
            "idempotency key is required".into(),
        ));
    }
    Ok(())
}

pub(super) async fn insert_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
    object_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, created_at) VALUES (?, ?, ?, 'plugin_command', 'plugin', ?, 'pending', ?)",
        vec![scope.into(), key.into(), digest.into(), object_id.into(), now.into()],
    )).await?;
    Ok(())
}

pub(super) async fn finish_receipt<C: ConnectionTrait, T: Serialize>(
    connection: &C,
    scope: &str,
    key: &str,
    value: &T,
    now: i64,
) -> StorageResult<()> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| StorageError::Integrity(error.to_string()))?;
    let object_id = put_inline_object(connection, &bytes, now).await?;
    connection.execute_raw(sql(
        "UPDATE application_command_receipts SET status = 'completed', result_object_id = ?, completed_at = ? WHERE scope = ? AND idempotency_key = ? AND status = 'pending'",
        vec![object_id.into(), now.into(), scope.into(), key.into()],
    )).await?;
    Ok(())
}

pub(super) fn result_id(value: Option<String>) -> StorageResult<String> {
    value.ok_or_else(|| StorageError::Integrity("plugin receipt has no result object".into()))
}
