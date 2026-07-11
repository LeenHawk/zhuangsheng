use sea_orm::ConnectionTrait;
use serde::{Serialize, de::DeserializeOwned};
use zhuangsheng_core::canonical;

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
};

pub(super) async fn replay<T: DeserializeOwned, C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<T>> {
    let row = connection.query_one(sql(
        "SELECT request_digest, status, result_object_id FROM application_command_receipts WHERE scope = ? AND idempotency_key = ?",
        vec![scope.into(), key.into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    if row.try_get::<String>("", "status")? == "expired" {
        return Err(StorageError::Conflict("idempotency_key_expired"));
    }
    let object_id: String = row.try_get("", "result_object_id")?;
    Ok(Some(load_object_json(connection, &object_id).await?))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn finish<T: Serialize, C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
    command_kind: &str,
    resource_kind: &str,
    resource_id: &str,
    result: &T,
    now: i64,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(result)?, now).await?;
    connection.execute(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES (?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?)",
        vec![scope.into(), key.into(), digest.into(), command_kind.into(), resource_kind.into(), resource_id.into(), object_id.clone().into(), now.into(), now.into()],
    )).await?;
    connection.execute(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'application_receipt', ?, 'result', ?)",
        vec![object_id.into(), format!("{scope}:{key}").into(), now.into()],
    )).await?;
    Ok(())
}
