use async_trait::async_trait;
use sea_orm::TransactionTrait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        secret::{SecretResolver, SecretValue},
    },
    llm::SecretRef,
};

use crate::{SqliteStore, StorageResult, graph::helpers::now_ms};

use super::*;

impl SqliteStore {
    async fn resolve_secret_value(&self, secret_ref: &SecretRef) -> StorageResult<SecretValue> {
        secret_ref
            .validate()
            .map_err(|error| SecretStoreError::InvalidArgument(error.message))?;
        let now = now_ms();
        let mut sessions = self.secret_sessions.lock().await;
        let active = sessions.active(None, now)?;
        let header = load_header(&self.db)
            .await?
            .ok_or(SecretStoreError::NotInitialized)?;
        if active.session.store_id != header.store_id {
            return Err(SecretStoreError::CorruptStore.into());
        }
        let record = load_secret_record(&self.db, &secret_ref.id).await?;
        let plaintext = decrypt_secret(
            &active.data_key,
            &header.store_id,
            &record.id,
            record.kind,
            &record.nonce,
            &record.ciphertext,
        )?;
        let transaction = self.db.begin().await?;
        append_secret_audit(
            &transaction,
            Some(&header.store_id),
            "secret_resolved",
            Some(&record.id),
            "succeeded",
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(SecretValue::from_zeroizing(plaintext))
    }
}

#[async_trait]
impl SecretResolver for SqliteStore {
    async fn resolve_secret(
        &self,
        secret_ref: &SecretRef,
    ) -> Result<SecretValue, ApplicationError> {
        self.resolve_secret_value(secret_ref)
            .await
            .map_err(Into::into)
    }
}
