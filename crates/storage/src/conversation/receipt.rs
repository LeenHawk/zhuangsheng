use sea_orm::ConnectionTrait;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
};
use zhuangsheng_core::canonical;

pub(super) async fn replay<C: ConnectionTrait, T: DeserializeOwned>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<T>> {
    let row = connection.query_one_raw(sql(
        "SELECT request_digest, result_object_id FROM application_command_receipts WHERE scope = ? AND idempotency_key = ? AND status = 'completed'",
        vec![scope.into(), key.into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    load_object_json(connection, &row.try_get::<String>("", "result_object_id")?)
        .await
        .map(Some)
}

pub(super) struct Receipt<'a> {
    pub scope: &'a str,
    pub key: &'a str,
    pub digest: &'a str,
    pub command_kind: &'a str,
    pub resource_kind: &'a str,
    pub resource_id: &'a str,
    pub now: i64,
}

pub(super) async fn finish<C: ConnectionTrait, T: Serialize>(
    connection: &C,
    receipt: Receipt<'_>,
    result: &T,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(result)?, receipt.now).await?;
    connection.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES (?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?)",
        vec![receipt.scope.into(), receipt.key.into(), receipt.digest.into(), receipt.command_kind.into(), receipt.resource_kind.into(), receipt.resource_id.into(), object_id.clone().into(), receipt.now.into(), receipt.now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'application_receipt', ?, 'result', ?)",
        vec![object_id.into(), format!("{}:{}", receipt.scope, receipt.key).into(), receipt.now.into()],
    )).await?;
    Ok(())
}
