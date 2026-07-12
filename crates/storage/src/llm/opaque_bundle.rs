use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::llm::{
    LlmOperationExecutionPin,
    adapter::{SensitiveEntryDraft, resolve_shape_adapter},
    ir::OpaqueContinuationRef,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, sql},
    secret::{SecretStoreError, encrypt_internal_bundle, load_header},
};

pub use super::opaque_bundle_format::StoredOpaqueBundleRefs;
use super::{
    opaque_bundle_format::{
        MAX_BUNDLE_BYTES, MAX_ENTRY_BYTES, build_bundle, bundle_refs, validate_bundle,
        validate_reference,
    },
    opaque_bundle_rows::{
        ciphertext_digest, decrypt_bundle, load_cipher_by_effect, load_cipher_by_id,
    },
};

impl SqliteStore {
    pub async fn store_llm_opaque_bundle(
        &self,
        effect_attempt_id: &str,
        model_call_id: &str,
        operation: &LlmOperationExecutionPin,
        entries: &[SensitiveEntryDraft],
        now: i64,
    ) -> StorageResult<StoredOpaqueBundleRefs> {
        let adapter = resolve_shape_adapter(operation)
            .map_err(|error| StorageError::InvalidArgument(error.message))?
            .key;
        let bundle = build_bundle(
            effect_attempt_id,
            model_call_id,
            operation,
            adapter,
            entries,
        )?;
        let plaintext = zhuangsheng_core::canonical::to_vec(&bundle)?;
        if plaintext.len() > MAX_BUNDLE_BYTES {
            return Err(StorageError::InvalidArgument(
                "opaque bundle exceeds 1 MiB".into(),
            ));
        }
        let (header, active) = self.active_bundle_key(now).await?;
        let transaction = self.db.begin().await?;
        if let Some(stored) = load_cipher_by_effect(&transaction, effect_attempt_id).await? {
            let existing = decrypt_bundle(&active.data_key, &header.store_id, &stored)?;
            if existing != bundle {
                return Err(StorageError::Conflict("opaque_bundle_replay"));
            }
            transaction.commit().await?;
            return Ok(bundle_refs(&stored.object_id, &stored.digest, &bundle));
        }
        let object_id = new_id("sensitive");
        let encrypted = encrypt_internal_bundle(
            &active.data_key,
            &header.store_id,
            effect_attempt_id,
            &object_id,
            &plaintext,
        )?;
        let digest = ciphertext_digest(&encrypted.nonce, &encrypted.ciphertext);
        transaction.execute_raw(sql(
            "INSERT INTO internal_sensitive_objects (id, origin_effect_attempt_id, format_version, ciphertext_digest, byte_size, purpose, key_version, kdf_version, algorithm, lifecycle, lifecycle_generation, nonce, ciphertext, created_at) VALUES (?, ?, 1, ?, ?, 'provider_opaque_bundle_v1', 1, 1, 'xchacha20-poly1305', 'live', 1, ?, ?, ?)",
            vec![object_id.clone().into(), effect_attempt_id.into(), digest.clone().into(), (encrypted.ciphertext.len() as i64).into(), encrypted.nonce.into(), encrypted.ciphertext.into(), now.into()],
        )).await?;
        transaction.commit().await?;
        Ok(bundle_refs(&object_id, &digest, &bundle))
    }

    pub async fn load_llm_opaque_entries(
        &self,
        references: &[OpaqueContinuationRef],
        operation: &LlmOperationExecutionPin,
        now: i64,
    ) -> StorageResult<BTreeMap<String, Vec<u8>>> {
        if references.is_empty() {
            return Ok(BTreeMap::new());
        }
        let expected_adapter = resolve_shape_adapter(operation)
            .map_err(|error| StorageError::InvalidArgument(error.message))?
            .key
            .as_str();
        let (header, active) = self.active_bundle_key(now).await?;
        let mut objects = BTreeMap::new();
        let mut result = BTreeMap::new();
        for reference in references {
            validate_reference(reference, operation, expected_adapter, now)?;
            if !objects.contains_key(&reference.entry_ref.object_id) {
                let stored = load_cipher_by_id(&self.db, &reference.entry_ref.object_id).await?;
                if stored.digest != reference.digest {
                    return Err(SecretStoreError::CorruptStore.into());
                }
                let bundle = decrypt_bundle(&active.data_key, &header.store_id, &stored)?;
                validate_bundle(
                    &bundle,
                    &stored.effect_attempt_id,
                    operation,
                    expected_adapter,
                )?;
                objects.insert(stored.object_id, bundle);
            }
            let bundle = objects
                .get(&reference.entry_ref.object_id)
                .ok_or_else(|| StorageError::Integrity("opaque bundle disappeared".into()))?;
            if bundle.model_call_id != reference.model_call_id {
                return Err(SecretStoreError::CorruptStore.into());
            }
            let entry = bundle
                .entries
                .get(&reference.entry_ref.entry_key)
                .ok_or(SecretStoreError::CorruptStore)?;
            let bytes = URL_SAFE_NO_PAD
                .decode(&entry.bytes_base64)
                .map_err(|_| SecretStoreError::CorruptStore)?;
            if bytes.is_empty() || bytes.len() > MAX_ENTRY_BYTES {
                return Err(SecretStoreError::CorruptStore.into());
            }
            result.insert(
                format!(
                    "{}:{}",
                    reference.entry_ref.object_id, reference.entry_ref.entry_key
                ),
                bytes,
            );
        }
        Ok(result)
    }

    async fn active_bundle_key(
        &self,
        now: i64,
    ) -> StorageResult<(
        crate::secret::SecretStoreHeader,
        crate::secret::ActiveSessionKey,
    )> {
        let header = load_header(&self.db)
            .await?
            .ok_or(SecretStoreError::NotInitialized)?;
        let active = {
            let mut sessions = self.secret_sessions.lock().await;
            sessions.active(None, now)?
        };
        if active.session.store_id != header.store_id {
            return Err(SecretStoreError::CorruptStore.into());
        }
        Ok((header, active))
    }
}
