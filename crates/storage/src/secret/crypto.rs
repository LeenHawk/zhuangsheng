use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;
use zhuangsheng_core::{application::secret::SecretKind, canonical};

use super::SecretStoreError;

pub const SECRET_STORE_MAGIC: &str = "zhuangsheng-secret-store";
pub const SECRET_STORE_FORMAT_VERSION: u32 = 1;
const DATA_KEY_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const KDF_MEMORY_KIB: u32 = 65_536;
const KDF_ITERATIONS: u32 = 3;
const KDF_PARALLELISM: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SecretStoreHeader {
    pub magic: String,
    pub format_version: u32,
    pub store_id: String,
    pub kdf: SecretKdfSpec,
    pub key_wrap: SecretKeyWrap,
    pub active_key_version: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SecretKdfSpec {
    pub algorithm: String,
    pub version: u32,
    pub salt: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SecretKeyWrap {
    pub algorithm: String,
    pub nonce: String,
    pub wrapped_data_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyWrapAad<'a> {
    magic: &'a str,
    format_version: u32,
    store_id: &'a str,
    kdf: &'a SecretKdfSpec,
    wrap_algorithm: &'a str,
    active_key_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordAad<'a> {
    format_version: u32,
    store_id: &'a str,
    record_id: &'a str,
    kind: SecretKind,
    key_version: u32,
}

pub(crate) struct EncryptedSecret {
    pub nonce: String,
    pub ciphertext: Vec<u8>,
}

pub(crate) fn create_header(
    password: &[u8],
    store_id: &str,
    now: i64,
) -> Result<(SecretStoreHeader, Zeroizing<[u8; 32]>), SecretStoreError> {
    validate_password(password)?;
    let salt = random_array::<SALT_BYTES>()?;
    let nonce = random_array::<NONCE_BYTES>()?;
    let data_key = Zeroizing::new(random_array::<DATA_KEY_BYTES>()?);
    let kdf = SecretKdfSpec {
        algorithm: "argon2id".into(),
        version: 19,
        salt: encode(&salt),
        memory_kib: KDF_MEMORY_KIB,
        iterations: KDF_ITERATIONS,
        parallelism: KDF_PARALLELISM,
    };
    let mut header = SecretStoreHeader {
        magic: SECRET_STORE_MAGIC.into(),
        format_version: SECRET_STORE_FORMAT_VERSION,
        store_id: store_id.into(),
        kdf,
        key_wrap: SecretKeyWrap {
            algorithm: "xchacha20-poly1305".into(),
            nonce: encode(&nonce),
            wrapped_data_key: String::new(),
        },
        active_key_version: 1,
        created_at: now,
        updated_at: now,
    };
    let kek = derive_kek(password, &header.kdf)?;
    let aad = key_wrap_aad(&header)?;
    header.key_wrap.wrapped_data_key = encode(&encrypt(&kek[..], &nonce, &data_key[..], &aad)?);
    Ok((header, data_key))
}

pub(crate) fn unlock_header(
    password: &[u8],
    header: &SecretStoreHeader,
) -> Result<Zeroizing<[u8; 32]>, SecretStoreError> {
    validate_header(header)?;
    let kek = derive_kek(password, &header.kdf).map_err(|_| SecretStoreError::UnlockFailed)?;
    let nonce = decode_array::<NONCE_BYTES>(&header.key_wrap.nonce)
        .map_err(|_| SecretStoreError::UnlockFailed)?;
    let ciphertext =
        decode(&header.key_wrap.wrapped_data_key).map_err(|_| SecretStoreError::UnlockFailed)?;
    let aad = key_wrap_aad(header).map_err(|_| SecretStoreError::UnlockFailed)?;
    let plaintext =
        decrypt(&kek[..], &nonce, &ciphertext, &aad).map_err(|_| SecretStoreError::UnlockFailed)?;
    if plaintext.len() != DATA_KEY_BYTES {
        return Err(SecretStoreError::UnlockFailed);
    }
    let mut key = [0_u8; DATA_KEY_BYTES];
    key.copy_from_slice(&plaintext);
    Ok(Zeroizing::new(key))
}

pub(crate) fn rewrap_header(
    current: &SecretStoreHeader,
    new_password: &[u8],
    data_key: &[u8; 32],
    now: i64,
) -> Result<SecretStoreHeader, SecretStoreError> {
    validate_header(current)?;
    validate_password(new_password)?;
    let salt = random_array::<SALT_BYTES>()?;
    let nonce = random_array::<NONCE_BYTES>()?;
    let kdf = SecretKdfSpec {
        algorithm: "argon2id".into(),
        version: 19,
        salt: encode(&salt),
        memory_kib: KDF_MEMORY_KIB,
        iterations: KDF_ITERATIONS,
        parallelism: KDF_PARALLELISM,
    };
    let mut header = SecretStoreHeader {
        magic: current.magic.clone(),
        format_version: current.format_version,
        store_id: current.store_id.clone(),
        kdf,
        key_wrap: SecretKeyWrap {
            algorithm: "xchacha20-poly1305".into(),
            nonce: encode(&nonce),
            wrapped_data_key: String::new(),
        },
        active_key_version: current.active_key_version,
        created_at: current.created_at,
        updated_at: now,
    };
    let kek = derive_kek(new_password, &header.kdf)?;
    let aad = key_wrap_aad(&header)?;
    header.key_wrap.wrapped_data_key = encode(&encrypt(&kek[..], &nonce, data_key, &aad)?);
    Ok(header)
}

pub(crate) fn encrypt_secret(
    data_key: &[u8; 32],
    store_id: &str,
    record_id: &str,
    kind: SecretKind,
    plaintext: &[u8],
) -> Result<EncryptedSecret, SecretStoreError> {
    if plaintext.is_empty() || plaintext.len() > 64 * 1024 {
        return Err(SecretStoreError::InvalidArgument(
            "secret value must contain 1..=65536 bytes".into(),
        ));
    }
    let nonce = random_array::<NONCE_BYTES>()?;
    let aad = canonical::to_vec(&RecordAad {
        format_version: 1,
        store_id,
        record_id,
        kind,
        key_version: 1,
    })
    .map_err(|_| SecretStoreError::Crypto)?;
    Ok(EncryptedSecret {
        nonce: encode(&nonce),
        ciphertext: encrypt(data_key, &nonce, plaintext, &aad)?,
    })
}

pub(crate) fn decrypt_secret(
    data_key: &[u8; 32],
    store_id: &str,
    record_id: &str,
    kind: SecretKind,
    nonce: &str,
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SecretStoreError> {
    let nonce = decode_array::<NONCE_BYTES>(nonce).map_err(|_| SecretStoreError::CorruptStore)?;
    let aad = canonical::to_vec(&RecordAad {
        format_version: 1,
        store_id,
        record_id,
        kind,
        key_version: 1,
    })
    .map_err(|_| SecretStoreError::CorruptStore)?;
    decrypt(data_key, &nonce, ciphertext, &aad)
        .map(Zeroizing::new)
        .map_err(|_| SecretStoreError::CorruptStore)
}

pub(crate) fn receipt_hmac(
    data_key: &[u8; 32],
    store_id: &str,
    fields: &[&[u8]],
) -> Result<Vec<u8>, SecretStoreError> {
    let hkdf = Hkdf::<Sha256>::new(Some(store_id.as_bytes()), data_key);
    let mut receipt_key = Zeroizing::new([0_u8; 32]);
    hkdf.expand(b"zhuangsheng/secret-command-receipt/v1", &mut *receipt_key)
        .map_err(|_| SecretStoreError::Crypto)?;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&receipt_key[..])
        .map_err(|_| SecretStoreError::Crypto)?;
    mac.update(b"zhuangsheng/secret-command/v1");
    for field in fields {
        mac.update(&(field.len() as u64).to_be_bytes());
        mac.update(field);
    }
    Ok(mac.finalize().into_bytes().to_vec())
}

pub(crate) fn verify_receipt_hmac(expected: &[u8], actual: &[u8]) -> bool {
    expected.len() == actual.len() && bool::from(expected.ct_eq(actual))
}

fn validate_password(password: &[u8]) -> Result<(), SecretStoreError> {
    if password.len() < 12 || password.len() > 1024 {
        return Err(SecretStoreError::InvalidArgument(
            "master password must contain 12..=1024 bytes".into(),
        ));
    }
    Ok(())
}

fn validate_header(header: &SecretStoreHeader) -> Result<(), SecretStoreError> {
    if header.magic != SECRET_STORE_MAGIC
        || header.format_version != 1
        || header.active_key_version != 1
        || header.kdf.algorithm != "argon2id"
        || header.kdf.version != 19
        || header.key_wrap.algorithm != "xchacha20-poly1305"
    {
        return Err(SecretStoreError::UnsupportedFormat);
    }
    if header.kdf.memory_kib < KDF_MEMORY_KIB
        || header.kdf.memory_kib > 1024 * 1024
        || header.kdf.iterations < KDF_ITERATIONS
        || header.kdf.iterations > 10
        || header.kdf.parallelism == 0
        || header.kdf.parallelism > 8
    {
        return Err(SecretStoreError::UnsupportedFormat);
    }
    Ok(())
}

fn derive_kek(
    password: &[u8],
    kdf: &SecretKdfSpec,
) -> Result<Zeroizing<[u8; 32]>, SecretStoreError> {
    let salt = decode(&kdf.salt).map_err(|_| SecretStoreError::CorruptStore)?;
    let params = Params::new(kdf.memory_kib, kdf.iterations, kdf.parallelism, Some(32))
        .map_err(|_| SecretStoreError::UnsupportedFormat)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = Zeroizing::new([0_u8; 32]);
    argon
        .hash_password_into(password, &salt, &mut *output)
        .map_err(|_| SecretStoreError::Crypto)?;
    Ok(output)
}

fn key_wrap_aad(header: &SecretStoreHeader) -> Result<Vec<u8>, SecretStoreError> {
    canonical::to_vec(&KeyWrapAad {
        magic: &header.magic,
        format_version: header.format_version,
        store_id: &header.store_id,
        kdf: &header.kdf,
        wrap_algorithm: &header.key_wrap.algorithm,
        active_key_version: header.active_key_version,
    })
    .map_err(|_| SecretStoreError::Crypto)
}

pub(super) fn encrypt(
    key: &[u8],
    nonce: &[u8; NONCE_BYTES],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, SecretStoreError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| SecretStoreError::Crypto)?;
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| SecretStoreError::Crypto)
}

pub(super) fn decrypt(
    key: &[u8],
    nonce: &[u8; NONCE_BYTES],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, SecretStoreError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| SecretStoreError::Crypto)?;
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| SecretStoreError::Crypto)
}

pub(super) fn random_array<const N: usize>() -> Result<[u8; N], SecretStoreError> {
    let mut value = [0_u8; N];
    getrandom::fill(&mut value).map_err(|_| SecretStoreError::Crypto)?;
    Ok(value)
}

pub(super) fn encode(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

fn decode(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    URL_SAFE_NO_PAD.decode(value)
}

pub(super) fn decode_array<const N: usize>(value: &str) -> Result<[u8; N], base64::DecodeError> {
    let decoded = decode(value)?;
    let length = decoded.len();
    decoded
        .try_into()
        .map_err(|_| base64::DecodeError::InvalidLength(length))
}
