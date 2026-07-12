use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;
use zhuangsheng_core::application::secret::{
    InitializeSecretStoreCommand, PutSecretCommand, SecretKind, SecretMetadataView,
    SecretStoreSessionView, SecretStoreStatusView, SecretValue, UnlockSecretStoreCommand,
};

use crate::{CommandResult, TauriAdapter};

pub struct SensitiveSecretInput {
    master_password: Zeroizing<String>,
    idempotency_key: String,
}

pub type SensitivePasswordInput = SensitiveSecretInput;

pub struct SensitivePutSecretInput {
    secret_id: String,
    name: Option<String>,
    kind: SecretKind,
    value: Zeroizing<String>,
    session_id: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SensitiveSecretWire {
    master_password: String,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SensitivePutSecretWire {
    secret_id: String,
    name: Option<String>,
    kind: SecretKind,
    value: String,
    session_id: String,
    idempotency_key: String,
}

impl<'de> Deserialize<'de> for SensitiveSecretInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SensitiveSecretWire::deserialize(deserializer)?;
        Ok(Self {
            master_password: Zeroizing::new(wire.master_password),
            idempotency_key: wire.idempotency_key,
        })
    }
}

impl<'de> Deserialize<'de> for SensitivePutSecretInput {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = SensitivePutSecretWire::deserialize(deserializer)?;
        Ok(Self {
            secret_id: wire.secret_id,
            name: wire.name,
            kind: wire.kind,
            value: Zeroizing::new(wire.value),
            session_id: wire.session_id,
            idempotency_key: wire.idempotency_key,
        })
    }
}

impl TauriAdapter {
    pub async fn get_secret_store_status(&self) -> CommandResult<SecretStoreStatusView> {
        Ok(self.secret.status().await?)
    }

    pub async fn list_secrets(&self) -> CommandResult<Vec<SecretMetadataView>> {
        Ok(self.secret.list_secrets().await?)
    }

    pub async fn put_secret(
        &self,
        input: SensitivePutSecretInput,
    ) -> CommandResult<SecretMetadataView> {
        Ok(self
            .secret
            .put_secret(PutSecretCommand {
                secret_id: input.secret_id,
                name: input.name,
                kind: input.kind,
                value: SecretValue::from_utf8(input.value.to_string()),
                session_id: input.session_id,
                idempotency_key: input.idempotency_key,
            })
            .await?)
    }

    pub async fn initialize_secret_store(
        &self,
        input: SensitiveSecretInput,
    ) -> CommandResult<SecretStoreSessionView> {
        Ok(self
            .secret
            .initialize(InitializeSecretStoreCommand {
                master_password: SecretValue::from_utf8(input.master_password.to_string()),
                idempotency_key: input.idempotency_key,
            })
            .await?)
    }

    pub async fn unlock_secret_store(
        &self,
        input: SensitiveSecretInput,
    ) -> CommandResult<SecretStoreSessionView> {
        Ok(self
            .secret
            .unlock(UnlockSecretStoreCommand {
                master_password: SecretValue::from_utf8(input.master_password.to_string()),
                idempotency_key: input.idempotency_key,
            })
            .await?)
    }
}
