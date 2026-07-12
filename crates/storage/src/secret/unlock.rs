use sea_orm::TransactionTrait;
use zhuangsheng_core::application::secret::*;

use crate::{SqliteStore, StorageResult, graph::helpers::*};

use super::*;

const SCOPE: &str = "workspace:local:secret-store:unlock";
const COMMAND: &str = "unlock_secret_store";

impl SqliteStore {
    pub async fn unlock_secret_store(
        &self,
        command: UnlockSecretStoreCommand,
    ) -> StorageResult<SecretStoreSessionView> {
        require_idempotency_key(&command.idempotency_key)?;
        let now = now_ms();
        let mut sessions = self.secret_sessions.lock().await;
        let header = load_header(&self.db)
            .await?
            .ok_or(SecretStoreError::NotInitialized)?;
        if let Some(receipt) =
            find_secret_receipt(&self.db, SCOPE, &command.idempotency_key).await?
        {
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
            let request_hmac = unlock_hmac(
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
        sessions.check_unlock_rate(now)?;
        let data_key = match command
            .master_password
            .with_bytes(|password| unlock_header(password, &header))
        {
            Ok(data_key) => {
                sessions.record_unlock_success();
                data_key
            }
            Err(error) => {
                if matches!(error, SecretStoreError::UnlockFailed) {
                    sessions.record_unlock_failure(now);
                }
                return Err(error.into());
            }
        };
        let request_hmac = unlock_hmac(
            &data_key,
            &header.store_id,
            &command.idempotency_key,
            &command.master_password,
        )?;
        let session_id = new_id("secretsession");
        let process_generation = sessions.process_generation().to_owned();
        let expires_at = now.saturating_add(SESSION_IDLE_TIMEOUT_MS);
        let result = SecretStoreSessionView {
            store_id: header.store_id.clone(),
            format_version: 1,
            session_id: session_id.clone(),
            expires_at,
        };
        let transaction = self.db.begin().await?;
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
            Some(&header.store_id),
            "store_unlocked",
            None,
            "succeeded",
            now,
        )
        .await?;
        transaction.commit().await?;
        let installed = sessions.install(header.store_id, session_id, data_key, now);
        if installed != result {
            return Err(crate::StorageError::Integrity(
                "installed secret session differs from receipt".into(),
            ));
        }
        self.resolve_secret_unlock_waits(&result.session_id, now)
            .await?;
        Ok(result)
    }
}

fn unlock_hmac(
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
