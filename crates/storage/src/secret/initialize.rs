use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{application::secret::*, canonical};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::*;

const SCOPE: &str = "workspace:local:secret-store:initialize";
const COMMAND: &str = "initialize_secret_store";

impl SqliteStore {
    pub async fn initialize_secret_store(
        &self,
        command: InitializeSecretStoreCommand,
    ) -> StorageResult<SecretStoreSessionView> {
        require_idempotency_key(&command.idempotency_key)?;
        let now = now_ms();
        let mut sessions = self.secret_sessions.lock().await;
        let existing_header = load_header(&self.db).await?;
        if let Some(header) = existing_header {
            let Some(receipt) =
                find_secret_receipt(&self.db, SCOPE, &command.idempotency_key).await?
            else {
                return Err(SecretStoreError::AlreadyInitialized.into());
            };
            let session_id = receipt
                .unlock_session_id
                .as_deref()
                .ok_or(SecretStoreError::CorruptStore)?;
            let process = receipt
                .unlock_process_generation
                .as_deref()
                .ok_or(SecretStoreError::CorruptStore)?;
            if sessions.process_generation() != process
                || receipt
                    .result_expires_at
                    .is_some_and(|expiry| expiry <= now)
            {
                return Err(SecretStoreError::IdempotencyKeyExpired.into());
            }
            let active = sessions
                .active(Some(session_id), now)
                .map_err(|_| SecretStoreError::IdempotencyKeyExpired)?;
            let request_hmac = initialize_hmac(
                &active.data_key,
                &header.store_id,
                &command.idempotency_key,
                &command.master_password,
            )?;
            verify_receipt(&receipt, &request_hmac)?;
            let result = load_secret_result(&self.db, &receipt).await?;
            self.resolve_secret_unlock_waits(session_id, now).await?;
            return Ok(result);
        }
        let store_id = new_id("secretstore");
        let session_id = new_id("secretsession");
        let process_generation = sessions.process_generation().to_owned();
        let (header, data_key) = command
            .master_password
            .with_bytes(|password| create_header(password, &store_id, now))?;
        let expires_at = now.saturating_add(SESSION_IDLE_TIMEOUT_MS);
        let result = SecretStoreSessionView {
            store_id: store_id.clone(),
            format_version: 1,
            session_id: session_id.clone(),
            expires_at,
        };
        let request_hmac = initialize_hmac(
            &data_key,
            &store_id,
            &command.idempotency_key,
            &command.master_password,
        )?;
        let transaction = self.db.begin().await?;
        if load_header(&transaction).await?.is_some() {
            return Err(SecretStoreError::AlreadyInitialized.into());
        }
        transaction
            .execute_raw(sql(
                "INSERT INTO secret_store_headers (singleton, store_id, format_version, header_json, created_at, updated_at) VALUES (1, ?, 1, ?, ?, ?)",
                vec![
                    store_id.clone().into(),
                    canonical::to_string(&header)?.into(),
                    now.into(),
                    now.into(),
                ],
            ))
            .await?;
        insert_secret_receipt(
            &transaction,
            SCOPE,
            &command.idempotency_key,
            COMMAND,
            &request_hmac,
            &result,
            Some(&session_id),
            Some(&process_generation),
            Some(expires_at),
            now,
        )
        .await?;
        append_secret_audit(
            &transaction,
            Some(&store_id),
            "store_created",
            None,
            "succeeded",
            now,
        )
        .await?;
        transaction.commit().await?;
        let installed = sessions.install(store_id, session_id, data_key, now);
        if installed != result {
            return Err(StorageError::Integrity(
                "installed secret session differs from receipt".into(),
            ));
        }
        self.resolve_secret_unlock_waits(&result.session_id, now)
            .await?;
        Ok(result)
    }
}

fn initialize_hmac(
    data_key: &[u8; 32],
    store_id: &str,
    idempotency_key: &str,
    password: &SecretValue,
) -> Result<Vec<u8>, SecretStoreError> {
    password.with_bytes(|bytes| {
        receipt_hmac(
            data_key,
            store_id,
            &[
                SCOPE.as_bytes(),
                idempotency_key.as_bytes(),
                COMMAND.as_bytes(),
                b"{}",
                bytes,
            ],
        )
    })
}
