use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{application::secret::*, canonical};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::*;

impl SqliteStore {
    pub async fn secret_store_status(&self) -> StorageResult<SecretStoreStatusView> {
        let header = load_header(&self.db).await?;
        let mut sessions = self.secret_sessions.lock().await;
        Ok(match header {
            Some(header) => SecretStoreStatusView {
                initialized: true,
                store_id: Some(header.store_id),
                format_version: Some(header.format_version),
                locked: sessions.is_locked(now_ms()),
            },
            None => SecretStoreStatusView {
                initialized: false,
                store_id: None,
                format_version: None,
                locked: true,
            },
        })
    }

    pub async fn lock_secret_store(
        &self,
        command: LockSecretStoreCommand,
    ) -> StorageResult<LockSecretStoreResult> {
        require_idempotency_key(&command.idempotency_key)?;
        let scope = "workspace:local:secret-store:lock";
        let digest = canonical::hash(&json!({
            "command":"lock_secret_store",
            "expectedSessionId":command.expected_session_id,
        }))?;
        let now = now_ms();
        let mut sessions = self.secret_sessions.lock().await;
        let header = load_header(&self.db)
            .await?
            .ok_or(SecretStoreError::NotInitialized)?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let result = load_object_json(
                &transaction,
                receipt
                    .result_object_id
                    .as_deref()
                    .ok_or_else(|| StorageError::Integrity("lock receipt has no result".into()))?,
            )
            .await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let current_session = sessions.current_session_id(now);
        if command
            .expected_session_id
            .as_deref()
            .is_some_and(|expected| current_session.as_deref() != Some(expected))
        {
            return Err(SecretStoreError::SessionExpired.into());
        }
        let result = LockSecretStoreResult { locked: true };
        let result_object =
            put_inline_object(&transaction, &canonical::to_vec(&result)?, now).await?;
        transaction.execute_raw(sql(
            "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES (?, ?, ?, 'lock_secret_store', 'secret_store', ?, 'completed', ?, ?, ?)",
            vec![scope.into(), command.idempotency_key.into(), digest.into(), header.store_id.clone().into(), result_object.into(), now.into(), now.into()],
        )).await?;
        if let Some(session_id) = current_session {
            transaction.execute_raw(sql(
                "UPDATE secret_command_receipts SET status = 'expired', expired_at = ? WHERE unlock_session_id = ? AND status = 'completed'",
                vec![now.into(), session_id.into()],
            )).await?;
        }
        append_secret_audit(
            &transaction,
            Some(&header.store_id),
            "store_locked",
            None,
            "succeeded",
            now,
        )
        .await?;
        transaction.commit().await?;
        if !sessions.lock(command.expected_session_id.as_deref(), now) {
            return Err(StorageError::Integrity(
                "secret session changed while lock mutex was held".into(),
            ));
        }
        Ok(result)
    }
}
