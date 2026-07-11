use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::secret::{PutSecretCommand, SecretMetadataView},
    canonical,
    llm::{SecretRef, SecretScheme},
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::*;

impl SqliteStore {
    pub async fn list_secret_metadata(&self) -> StorageResult<Vec<SecretMetadataView>> {
        if load_header(&self.db).await?.is_none() {
            return Err(SecretStoreError::NotInitialized.into());
        }
        self.db
            .query_all(sql(
                "SELECT id, name, kind, created_at, updated_at FROM secret_records WHERE status = 'active' ORDER BY created_at, id",
                vec![],
            ))
            .await?
            .iter()
            .map(metadata_from_row)
            .collect()
    }

    pub async fn put_secret_record(
        &self,
        command: PutSecretCommand,
    ) -> StorageResult<SecretMetadataView> {
        require_idempotency_key(&command.idempotency_key)?;
        let secret_ref = SecretRef {
            scheme: SecretScheme::Secret,
            id: command.secret_id.clone(),
        };
        secret_ref
            .validate()
            .map_err(|error| SecretStoreError::InvalidArgument(error.message))?;
        let name = normalize_name(command.name.as_deref())?;
        let now = now_ms();
        let mut sessions = self.secret_sessions.lock().await;
        let active = sessions.active(Some(&command.session_id), now)?;
        let header = load_header(&self.db)
            .await?
            .ok_or(SecretStoreError::NotInitialized)?;
        if active.session.store_id != header.store_id {
            return Err(SecretStoreError::CorruptStore.into());
        }
        let scope = format!("workspace:local:secrets:{}:put", command.secret_id);
        let non_secret = canonical::to_vec(&json!({
            "secretId": command.secret_id,
            "name": name,
            "kind": command.kind,
        }))?;
        let request_hmac = command.value.with_bytes(|value| {
            receipt_hmac(
                &active.data_key,
                &header.store_id,
                &[
                    scope.as_bytes(),
                    command.idempotency_key.as_bytes(),
                    b"put_secret",
                    &non_secret,
                    value,
                ],
            )
        })?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) =
            find_secret_receipt(&transaction, &scope, &command.idempotency_key).await?
        {
            verify_receipt(&receipt, &request_hmac)?;
            let result = load_secret_result(&transaction, &receipt).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let encrypted = command.value.with_bytes(|value| {
            encrypt_secret(
                &active.data_key,
                &header.store_id,
                &command.secret_id,
                command.kind,
                value,
            )
        })?;
        upsert_record(
            &transaction,
            &header.store_id,
            &command,
            name.as_deref(),
            &encrypted,
            now,
        )
        .await?;
        let row = transaction
            .query_one(sql(
                "SELECT id, name, kind, created_at, updated_at FROM secret_records WHERE id = ? AND status = 'active'",
                vec![command.secret_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::Integrity("secret upsert did not persist".into()))?;
        let result = metadata_from_row(&row)?;
        insert_secret_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            "put_secret",
            &request_hmac,
            &result,
            None,
            None,
            None,
            now,
        )
        .await?;
        append_secret_audit(
            &transaction,
            Some(&header.store_id),
            "secret_written",
            Some(&command.secret_id),
            "succeeded",
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(result)
    }
}

async fn upsert_record<C: ConnectionTrait>(
    connection: &C,
    store_id: &str,
    command: &PutSecretCommand,
    name: Option<&str>,
    encrypted: &EncryptedSecret,
    now: i64,
) -> StorageResult<()> {
    connection
        .execute(sql(
            "INSERT INTO secret_records (id, store_id, name, kind, key_version, algorithm, nonce, ciphertext, status, created_at, updated_at) VALUES (?, ?, ?, ?, 1, 'xchacha20-poly1305', ?, ?, 'active', ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, kind = excluded.kind, key_version = 1, algorithm = excluded.algorithm, nonce = excluded.nonce, ciphertext = excluded.ciphertext, status = 'active', updated_at = excluded.updated_at, deleted_at = NULL WHERE secret_records.store_id = excluded.store_id",
            vec![
                command.secret_id.clone().into(),
                store_id.into(),
                name.map(str::to_owned).into(),
                kind_name(command.kind).into(),
                encrypted.nonce.clone().into(),
                encrypted.ciphertext.clone().into(),
                now.into(),
                now.into(),
            ],
        ))
        .await?;
    Ok(())
}

fn normalize_name(name: Option<&str>) -> Result<Option<String>, SecretStoreError> {
    let name = name
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if name.as_ref().is_some_and(|value| value.len() > 200) {
        return Err(SecretStoreError::InvalidArgument(
            "secret name exceeds 200 bytes".into(),
        ));
    }
    Ok(name)
}
