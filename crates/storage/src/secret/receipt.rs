use sea_orm::ConnectionTrait;
use serde::Serialize;
use zhuangsheng_core::canonical;

use crate::{StorageError, StorageResult, graph::helpers::*};

use super::{SecretCommandReceipt, SecretStoreError, verify_receipt_hmac};

pub(crate) fn require_idempotency_key(key: &str) -> Result<(), SecretStoreError> {
    if key.trim().is_empty() || key.len() > 256 {
        return Err(SecretStoreError::InvalidArgument(
            "idempotency key must contain 1..=256 bytes".into(),
        ));
    }
    Ok(())
}

pub(crate) fn verify_receipt(
    receipt: &SecretCommandReceipt,
    request_hmac: &[u8],
) -> StorageResult<()> {
    if !verify_receipt_hmac(&receipt.request_hmac, request_hmac) {
        return Err(StorageError::IdempotencyConflict);
    }
    if receipt.status == "expired" {
        return Err(SecretStoreError::IdempotencyKeyExpired.into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_secret_receipt<C: ConnectionTrait, T: Serialize>(
    connection: &C,
    scope: &str,
    idempotency_key: &str,
    command_kind: &str,
    request_hmac: &[u8],
    result: &T,
    unlock_session_id: Option<&str>,
    unlock_process_generation: Option<&str>,
    result_expires_at: Option<i64>,
    now: i64,
) -> StorageResult<()> {
    let result_object_id = put_inline_object(connection, &canonical::to_vec(result)?, now).await?;
    connection
        .execute(sql(
            "INSERT INTO secret_command_receipts (scope, idempotency_key, command_kind, receipt_key_version, request_hmac, status, result_object_id, unlock_session_id, unlock_process_generation, result_expires_at, created_at, completed_at) VALUES (?, ?, ?, 1, ?, 'completed', ?, ?, ?, ?, ?, ?)",
            vec![
                scope.into(),
                idempotency_key.into(),
                command_kind.into(),
                request_hmac.to_vec().into(),
                result_object_id.into(),
                unlock_session_id.map(str::to_owned).into(),
                unlock_process_generation.map(str::to_owned).into(),
                result_expires_at.into(),
                now.into(),
                now.into(),
            ],
        ))
        .await?;
    Ok(())
}

pub(crate) async fn load_secret_result<C: ConnectionTrait, T: serde::de::DeserializeOwned>(
    connection: &C,
    receipt: &SecretCommandReceipt,
) -> StorageResult<T> {
    load_object_json(
        connection,
        receipt
            .result_object_id
            .as_deref()
            .ok_or_else(|| StorageError::Integrity("secret receipt has no result".into()))?,
    )
    .await
}

pub(crate) async fn append_secret_audit<C: ConnectionTrait>(
    connection: &C,
    store_id: Option<&str>,
    action: &str,
    secret_id: Option<&str>,
    result: &str,
    now: i64,
) -> StorageResult<()> {
    connection
        .execute(sql(
            "INSERT INTO secret_store_audit (id, store_id, action, secret_id, result, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            vec![
                new_id("secretaudit").into(),
                store_id.map(str::to_owned).into(),
                action.into(),
                secret_id.map(str::to_owned).into(),
                result.into(),
                now.into(),
            ],
        ))
        .await?;
    Ok(())
}
