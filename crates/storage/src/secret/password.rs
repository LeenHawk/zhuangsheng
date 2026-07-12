use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use subtle::ConstantTimeEq;
use zhuangsheng_core::{application::secret::*, canonical};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::*;

impl SqliteStore {
    pub async fn change_secret_store_password(
        &self,
        command: ChangeMasterPasswordCommand,
    ) -> StorageResult<SecretStoreSessionView> {
        require_idempotency_key(&command.idempotency_key)?;
        let now = now_ms();
        let mut sessions = self.secret_sessions.lock().await;
        let active = sessions.active(Some(&command.session_id), now)?;
        let header = load_header(&self.db)
            .await?
            .ok_or(SecretStoreError::NotInitialized)?;
        let scope = "workspace:local:secret-store:change-password";
        let non_secret = canonical::to_vec(&json!({
            "storeId": header.store_id,
            "sessionId": command.session_id,
        }))?;
        let request_hmac = command.current_password.with_bytes(|current| {
            command.new_password.with_bytes(|new| {
                receipt_hmac(
                    &active.data_key,
                    &header.store_id,
                    &[
                        scope.as_bytes(),
                        command.idempotency_key.as_bytes(),
                        b"change_master_password",
                        &non_secret,
                        current,
                        new,
                    ],
                )
            })
        })?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) =
            find_secret_receipt(&transaction, scope, &command.idempotency_key).await?
        {
            verify_receipt(&receipt, &request_hmac)?;
            let result = load_secret_result(&transaction, &receipt).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        sessions.check_unlock_rate(now)?;
        let verified_key = match command
            .current_password
            .with_bytes(|password| unlock_header(password, &header))
        {
            Ok(key) => {
                sessions.record_unlock_success();
                key
            }
            Err(error) => {
                if matches!(error, SecretStoreError::UnlockFailed) {
                    sessions.record_unlock_failure(now);
                }
                return Err(error.into());
            }
        };
        if !equal_keys(&verified_key, &active.data_key) {
            return Err(SecretStoreError::UnlockFailed.into());
        }
        let updated_header = command.new_password.with_bytes(|new_password| {
            rewrap_header(&header, new_password, &active.data_key, now)
        })?;
        let updated = transaction
            .execute_raw(sql(
                "UPDATE secret_store_headers SET header_json = ?, updated_at = ? WHERE singleton = 1 AND store_id = ? AND updated_at = ?",
                vec![
                    canonical::to_string(&updated_header)?.into(),
                    now.into(),
                    header.store_id.clone().into(),
                    header.updated_at.into(),
                ],
            ))
            .await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("secret_store_header_changed"));
        }
        insert_secret_receipt(
            &transaction,
            scope,
            &command.idempotency_key,
            "change_master_password",
            &request_hmac,
            &active.session,
            None,
            None,
            None,
            now,
        )
        .await?;
        append_secret_audit(
            &transaction,
            Some(&header.store_id),
            "master_password_changed",
            None,
            "succeeded",
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(active.session)
    }
}

fn equal_keys(left: &[u8; 32], right: &[u8; 32]) -> bool {
    bool::from(left.ct_eq(right))
}
