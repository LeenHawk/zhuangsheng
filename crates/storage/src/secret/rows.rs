use sea_orm::{ConnectionTrait, QueryResult};
use zhuangsheng_core::{
    application::secret::{SecretKind, SecretMetadataView},
    llm::{SecretRef, SecretScheme},
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::{SECRET_STORE_FORMAT_VERSION, SecretStoreError, SecretStoreHeader};

pub(crate) struct SecretCommandReceipt {
    pub request_hmac: Vec<u8>,
    pub status: String,
    pub result_object_id: Option<String>,
    pub unlock_session_id: Option<String>,
    pub unlock_process_generation: Option<String>,
    pub result_expires_at: Option<i64>,
}

pub(crate) struct StoredSecretRecord {
    pub id: String,
    pub kind: SecretKind,
    pub nonce: String,
    pub ciphertext: Vec<u8>,
}

pub(crate) async fn load_header<C: ConnectionTrait>(
    connection: &C,
) -> StorageResult<Option<SecretStoreHeader>> {
    let Some(row) = connection
        .query_one_raw(sql(
            "SELECT store_id, format_version, header_json FROM secret_store_headers WHERE singleton = 1",
            vec![],
        ))
        .await?
    else {
        return Ok(None);
    };
    let format: i64 = row.try_get("", "format_version")?;
    if format != SECRET_STORE_FORMAT_VERSION as i64 {
        return Err(SecretStoreError::UnsupportedFormat.into());
    }
    let store_id: String = row.try_get("", "store_id")?;
    let json: String = row.try_get("", "header_json")?;
    let header: SecretStoreHeader = serde_json::from_str(&json)
        .map_err(|_| StorageError::SecretStore(SecretStoreError::CorruptStore))?;
    if header.store_id != store_id || header.format_version != format as u32 {
        return Err(SecretStoreError::CorruptStore.into());
    }
    Ok(Some(header))
}

pub(crate) async fn find_secret_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
) -> StorageResult<Option<SecretCommandReceipt>> {
    connection
        .query_one_raw(sql(
            "SELECT request_hmac, status, result_object_id, unlock_session_id, unlock_process_generation, result_expires_at FROM secret_command_receipts WHERE scope = ? AND idempotency_key = ?",
            vec![scope.into(), key.into()],
        ))
        .await?
        .map(|row| {
            Ok(SecretCommandReceipt {
                request_hmac: row.try_get("", "request_hmac")?,
                status: row.try_get("", "status")?,
                result_object_id: row.try_get("", "result_object_id")?,
                unlock_session_id: row.try_get("", "unlock_session_id")?,
                unlock_process_generation: row.try_get("", "unlock_process_generation")?,
                result_expires_at: row.try_get("", "result_expires_at")?,
            })
        })
        .transpose()
}

pub(crate) async fn load_secret_record<C: ConnectionTrait>(
    connection: &C,
    secret_id: &str,
) -> StorageResult<StoredSecretRecord> {
    let row = connection
        .query_one_raw(sql(
            "SELECT id, kind, key_version, algorithm, nonce, ciphertext FROM secret_records WHERE id = ? AND status = 'active'",
            vec![secret_id.into()],
        ))
        .await?
        .ok_or_else(|| SecretStoreError::NotFound(secret_id.into()))?;
    let key_version: i64 = row.try_get("", "key_version")?;
    let algorithm: String = row.try_get("", "algorithm")?;
    if key_version != 1 || algorithm != "xchacha20-poly1305" {
        return Err(SecretStoreError::UnsupportedFormat.into());
    }
    Ok(StoredSecretRecord {
        id: row.try_get("", "id")?,
        kind: decode_kind(&row)?,
        nonce: row.try_get("", "nonce")?,
        ciphertext: row.try_get("", "ciphertext")?,
    })
}

pub(crate) fn metadata_from_row(row: &QueryResult) -> StorageResult<SecretMetadataView> {
    let id: String = row.try_get("", "id")?;
    Ok(SecretMetadataView {
        secret_ref: SecretRef {
            scheme: SecretScheme::Secret,
            id,
        },
        name: row.try_get("", "name")?,
        kind: decode_kind(row)?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub(crate) fn kind_name(kind: SecretKind) -> &'static str {
    match kind {
        SecretKind::ApiKey => "api_key",
        SecretKind::Token => "token",
    }
}

fn decode_kind(row: &QueryResult) -> StorageResult<SecretKind> {
    match row.try_get::<String>("", "kind")?.as_str() {
        "api_key" => Ok(SecretKind::ApiKey),
        "token" => Ok(SecretKind::Token),
        _ => Err(SecretStoreError::CorruptStore.into()),
    }
}
