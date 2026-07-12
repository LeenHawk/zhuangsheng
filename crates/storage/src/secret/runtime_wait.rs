use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use zhuangsheng_core::{
    application::{
        ApplicationError,
        secret::{ResolveRuntimeSecretCommand, RuntimeSecretResolution, RuntimeSecretResolver},
    },
    llm::SecretRef,
};

use crate::{SqliteStore, StorageResult};

use super::SecretStoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SecretUnlockContinuation {
    pub schema_version: u32,
    pub node_instance_id: String,
    pub source_attempt_id: String,
    pub execution_snapshot_ref: String,
    pub read_set_digest: String,
}

impl SqliteStore {
    pub async fn resolve_runtime_secret_value(
        &self,
        secret_ref: &SecretRef,
        command: ResolveRuntimeSecretCommand,
        now: i64,
    ) -> StorageResult<RuntimeSecretResolution> {
        secret_ref
            .validate()
            .map_err(|error| SecretStoreError::InvalidArgument(error.message))?;
        let mut sessions = self.secret_sessions.lock().await;
        match sessions.active(None, now) {
            Ok(active) => {
                self.validate_runtime_secret_owner(&command, now).await?;
                Ok(RuntimeSecretResolution::Resolved(
                    self.resolve_secret_with_active(secret_ref, &active, now)
                        .await?,
                ))
            }
            Err(SecretStoreError::Locked) => {
                let wait_id = self.open_secret_unlock_wait(&command, now).await?;
                Ok(RuntimeSecretResolution::Waiting { wait_id })
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait]
impl RuntimeSecretResolver for SqliteStore {
    async fn resolve_runtime_secret(
        &self,
        secret_ref: &SecretRef,
        command: ResolveRuntimeSecretCommand,
        now_ms: i64,
    ) -> Result<RuntimeSecretResolution, ApplicationError> {
        self.resolve_runtime_secret_value(secret_ref, command, now_ms)
            .await
            .map_err(Into::into)
    }
}
