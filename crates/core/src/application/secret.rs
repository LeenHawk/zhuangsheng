use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::llm::SecretRef;

use super::ApplicationError;

pub struct SecretValue(Zeroizing<Vec<u8>>);

impl SecretValue {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn from_utf8(value: String) -> Self {
        Self::from_bytes(value.into_bytes())
    }

    pub fn from_zeroizing(bytes: Zeroizing<Vec<u8>>) -> Self {
        Self(bytes)
    }

    pub fn with_bytes<T>(&self, use_value: impl FnOnce(&[u8]) -> T) -> T {
        use_value(self.0.as_slice())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub struct InitializeSecretStoreCommand {
    pub master_password: SecretValue,
    pub idempotency_key: String,
}

pub struct UnlockSecretStoreCommand {
    pub master_password: SecretValue,
    pub idempotency_key: String,
}

pub struct PutSecretCommand {
    pub secret_id: String,
    pub name: Option<String>,
    pub kind: SecretKind,
    pub value: SecretValue,
    pub session_id: String,
    pub idempotency_key: String,
}

pub struct ChangeMasterPasswordCommand {
    pub current_password: SecretValue,
    pub new_password: SecretValue,
    pub session_id: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockSecretStoreCommand {
    pub expected_session_id: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    ApiKey,
    Token,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreStatusView {
    pub initialized: bool,
    pub store_id: Option<String>,
    pub format_version: Option<u32>,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStoreSessionView {
    pub store_id: String,
    pub format_version: u32,
    pub session_id: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadataView {
    pub secret_ref: SecretRef,
    pub name: Option<String>,
    pub kind: SecretKind,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockSecretStoreResult {
    pub locked: bool,
}

#[async_trait]
pub trait SecretStoreService: Send + Sync {
    async fn status(&self) -> Result<SecretStoreStatusView, ApplicationError>;
    async fn initialize(
        &self,
        command: InitializeSecretStoreCommand,
    ) -> Result<SecretStoreSessionView, ApplicationError>;
    async fn unlock(
        &self,
        command: UnlockSecretStoreCommand,
    ) -> Result<SecretStoreSessionView, ApplicationError>;
    async fn lock(
        &self,
        command: LockSecretStoreCommand,
    ) -> Result<LockSecretStoreResult, ApplicationError>;
    async fn list_secrets(&self) -> Result<Vec<SecretMetadataView>, ApplicationError>;
    async fn put_secret(
        &self,
        command: PutSecretCommand,
    ) -> Result<SecretMetadataView, ApplicationError>;
    async fn change_master_password(
        &self,
        command: ChangeMasterPasswordCommand,
    ) -> Result<SecretStoreSessionView, ApplicationError>;
}

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve_secret(&self, secret_ref: &SecretRef)
    -> Result<SecretValue, ApplicationError>;
}
