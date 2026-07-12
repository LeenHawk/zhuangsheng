use hkdf::Hkdf;
use serde::Serialize;
use sha2::Sha256;
use zeroize::Zeroizing;
use zhuangsheng_core::canonical;

use super::{SecretStoreError, crypto};

const NONCE_BYTES: usize = 24;
const MAX_BUNDLE_BYTES: usize = 1024 * 1024;
const PURPOSE: &str = "provider_opaque_bundle_v1";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InternalBundleAad<'a> {
    format_version: u32,
    store_id: &'a str,
    effect_attempt_id: &'a str,
    object_id: &'a str,
    purpose: &'static str,
    key_version: u32,
    kdf_version: u32,
    algorithm: &'static str,
}

pub(crate) struct EncryptedInternalBundle {
    pub nonce: String,
    pub ciphertext: Vec<u8>,
}

pub(crate) fn encrypt_internal_bundle(
    data_key: &[u8; 32],
    store_id: &str,
    effect_attempt_id: &str,
    object_id: &str,
    plaintext: &[u8],
) -> Result<EncryptedInternalBundle, SecretStoreError> {
    if plaintext.is_empty() || plaintext.len() > MAX_BUNDLE_BYTES {
        return Err(SecretStoreError::InvalidArgument(
            "internal sensitive bundle must contain 1..=1048576 bytes".into(),
        ));
    }
    let nonce = crypto::random_array::<NONCE_BYTES>()?;
    let key = internal_bundle_key(data_key, store_id, effect_attempt_id, object_id)?;
    let aad = internal_bundle_aad(store_id, effect_attempt_id, object_id)?;
    Ok(EncryptedInternalBundle {
        nonce: crypto::encode(&nonce),
        ciphertext: crypto::encrypt(&key[..], &nonce, plaintext, &aad)?,
    })
}

pub(crate) fn decrypt_internal_bundle(
    data_key: &[u8; 32],
    store_id: &str,
    effect_attempt_id: &str,
    object_id: &str,
    nonce: &str,
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SecretStoreError> {
    let nonce =
        crypto::decode_array::<NONCE_BYTES>(nonce).map_err(|_| SecretStoreError::CorruptStore)?;
    let key = internal_bundle_key(data_key, store_id, effect_attempt_id, object_id)
        .map_err(|_| SecretStoreError::CorruptStore)?;
    let aad = internal_bundle_aad(store_id, effect_attempt_id, object_id)
        .map_err(|_| SecretStoreError::CorruptStore)?;
    crypto::decrypt(&key[..], &nonce, ciphertext, &aad)
        .map(Zeroizing::new)
        .map_err(|_| SecretStoreError::CorruptStore)
}

fn internal_bundle_key(
    data_key: &[u8; 32],
    store_id: &str,
    effect_attempt_id: &str,
    object_id: &str,
) -> Result<Zeroizing<[u8; 32]>, SecretStoreError> {
    let hkdf = Hkdf::<Sha256>::new(Some(store_id.as_bytes()), data_key);
    let mut info =
        Vec::with_capacity(effect_attempt_id.len() + object_id.len() + PURPOSE.len() + 48);
    info.extend_from_slice(b"zhuangsheng/internal-sensitive/v1\0");
    info.extend_from_slice(effect_attempt_id.as_bytes());
    info.push(0);
    info.extend_from_slice(object_id.as_bytes());
    info.push(0);
    info.extend_from_slice(PURPOSE.as_bytes());
    let mut key = Zeroizing::new([0_u8; 32]);
    hkdf.expand(&info, &mut *key)
        .map_err(|_| SecretStoreError::Crypto)?;
    Ok(key)
}

fn internal_bundle_aad(
    store_id: &str,
    effect_attempt_id: &str,
    object_id: &str,
) -> Result<Vec<u8>, SecretStoreError> {
    canonical::to_vec(&InternalBundleAad {
        format_version: 1,
        store_id,
        effect_attempt_id,
        object_id,
        purpose: PURPOSE,
        key_version: 1,
        kdf_version: 1,
        algorithm: "xchacha20-poly1305",
    })
    .map_err(|_| SecretStoreError::Crypto)
}
