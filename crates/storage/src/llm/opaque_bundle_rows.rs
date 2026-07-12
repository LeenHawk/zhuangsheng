use sea_orm::ConnectionTrait;

use crate::{
    StorageResult,
    graph::helpers::sql,
    secret::{SecretStoreError, decrypt_internal_bundle},
};

use super::opaque_bundle_format::OpaqueBundle;

pub(super) struct StoredCipher {
    pub object_id: String,
    pub effect_attempt_id: String,
    pub digest: String,
    pub nonce: String,
    pub ciphertext: Vec<u8>,
}

pub(super) async fn load_cipher_by_effect<C: ConnectionTrait>(
    connection: &C,
    effect_attempt_id: &str,
) -> StorageResult<Option<StoredCipher>> {
    connection.query_one_raw(sql("SELECT id, origin_effect_attempt_id, ciphertext_digest, nonce, ciphertext, byte_size, format_version, key_version, kdf_version, algorithm, purpose FROM internal_sensitive_objects WHERE origin_effect_attempt_id = ? AND lifecycle = 'live'", vec![effect_attempt_id.into()])).await?.map(cipher_from_row).transpose()
}

pub(super) async fn load_cipher_by_id<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
) -> StorageResult<StoredCipher> {
    let row = connection.query_one_raw(sql("SELECT id, origin_effect_attempt_id, ciphertext_digest, nonce, ciphertext, byte_size, format_version, key_version, kdf_version, algorithm, purpose FROM internal_sensitive_objects WHERE id = ? AND lifecycle = 'live'", vec![object_id.into()])).await?.ok_or(SecretStoreError::CorruptStore)?;
    cipher_from_row(row)
}

fn cipher_from_row(row: sea_orm::QueryResult) -> StorageResult<StoredCipher> {
    let ciphertext: Vec<u8> = row.try_get("", "ciphertext")?;
    if row.try_get::<i64>("", "format_version")? != 1
        || row.try_get::<i64>("", "key_version")? != 1
        || row.try_get::<i64>("", "kdf_version")? != 1
        || row.try_get::<String>("", "algorithm")? != "xchacha20-poly1305"
        || row.try_get::<String>("", "purpose")? != "provider_opaque_bundle_v1"
        || row.try_get::<i64>("", "byte_size")? != ciphertext.len() as i64
    {
        return Err(SecretStoreError::CorruptStore.into());
    }
    let nonce: String = row.try_get("", "nonce")?;
    let digest: String = row.try_get("", "ciphertext_digest")?;
    if ciphertext_digest(&nonce, &ciphertext) != digest {
        return Err(SecretStoreError::CorruptStore.into());
    }
    Ok(StoredCipher {
        object_id: row.try_get("", "id")?,
        effect_attempt_id: row.try_get("", "origin_effect_attempt_id")?,
        digest,
        nonce,
        ciphertext,
    })
}

pub(super) fn decrypt_bundle(
    data_key: &[u8; 32],
    store_id: &str,
    stored: &StoredCipher,
) -> StorageResult<OpaqueBundle> {
    let plaintext = decrypt_internal_bundle(
        data_key,
        store_id,
        &stored.effect_attempt_id,
        &stored.object_id,
        &stored.nonce,
        &stored.ciphertext,
    )?;
    serde_json::from_slice(&plaintext).map_err(|_| SecretStoreError::CorruptStore.into())
}

pub(super) fn ciphertext_digest(nonce: &str, ciphertext: &[u8]) -> String {
    let mut container = Vec::with_capacity(nonce.len() + ciphertext.len() + 40);
    container.extend_from_slice(b"provider-opaque-ciphertext/v1\0");
    container.extend_from_slice(nonce.as_bytes());
    container.push(0);
    container.extend_from_slice(ciphertext);
    zhuangsheng_core::canonical::hash_bytes(&container)
}
