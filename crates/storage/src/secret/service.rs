use async_trait::async_trait;
use zhuangsheng_core::application::{
    ApplicationError,
    secret::{
        ChangeMasterPasswordCommand, InitializeSecretStoreCommand, LockSecretStoreCommand,
        LockSecretStoreResult, PutSecretCommand, SecretMetadataView, SecretStoreService,
        SecretStoreSessionView, SecretStoreStatusView, UnlockSecretStoreCommand,
    },
};

use crate::SqliteStore;

#[async_trait]
impl SecretStoreService for SqliteStore {
    async fn status(&self) -> Result<SecretStoreStatusView, ApplicationError> {
        self.secret_store_status().await.map_err(Into::into)
    }

    async fn initialize(
        &self,
        command: InitializeSecretStoreCommand,
    ) -> Result<SecretStoreSessionView, ApplicationError> {
        self.initialize_secret_store(command)
            .await
            .map_err(Into::into)
    }

    async fn unlock(
        &self,
        command: UnlockSecretStoreCommand,
    ) -> Result<SecretStoreSessionView, ApplicationError> {
        self.unlock_secret_store(command).await.map_err(Into::into)
    }

    async fn lock(
        &self,
        command: LockSecretStoreCommand,
    ) -> Result<LockSecretStoreResult, ApplicationError> {
        self.lock_secret_store(command).await.map_err(Into::into)
    }

    async fn list_secrets(&self) -> Result<Vec<SecretMetadataView>, ApplicationError> {
        self.list_secret_metadata().await.map_err(Into::into)
    }

    async fn put_secret(
        &self,
        command: PutSecretCommand,
    ) -> Result<SecretMetadataView, ApplicationError> {
        self.put_secret_record(command).await.map_err(Into::into)
    }

    async fn change_master_password(
        &self,
        command: ChangeMasterPasswordCommand,
    ) -> Result<SecretStoreSessionView, ApplicationError> {
        self.change_secret_store_password(command)
            .await
            .map_err(Into::into)
    }
}
