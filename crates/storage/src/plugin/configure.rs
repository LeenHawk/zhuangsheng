use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{application::plugin::*, canonical};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::{
    receipt::{finish_receipt, insert_receipt, require_key, result_id},
    rows::{load_installation, load_version, policy_name},
};

impl SqliteStore {
    pub async fn configure_plugin_installation(
        &self,
        command: ConfigurePluginCommand,
    ) -> StorageResult<PluginInstallationView> {
        require_key(&command.idempotency_key)?;
        let scope = format!("workspace:local:plugins:{}:configure", command.plugin_id);
        let digest = canonical::hash(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(value) = replay(&transaction, &scope, &command.idempotency_key, &digest).await?
        {
            transaction.commit().await?;
            return Ok(value);
        }
        let current = load_installation(&transaction, &command.plugin_id).await?;
        let now = now_ms();
        insert_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &command.plugin_id,
            now,
        )
        .await?;
        transaction.execute_raw(sql(
            "UPDATE plugin_installations SET enabled = ?, update_policy = ?, updated_at = ? WHERE plugin_id = ?",
            vec![i64::from(command.enabled).into(), policy_name(command.update_policy).into(), now.into(), command.plugin_id.clone().into()],
        )).await?;
        let view = load_installation(&transaction, &current.plugin_id).await?;
        finish_receipt(&transaction, &scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn rollback_plugin_installation(
        &self,
        command: RollbackPluginCommand,
    ) -> StorageResult<PluginInstallationView> {
        require_key(&command.idempotency_key)?;
        let scope = format!("workspace:local:plugins:{}:rollback", command.plugin_id);
        let digest = canonical::hash(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(value) = replay(&transaction, &scope, &command.idempotency_key, &digest).await?
        {
            transaction.commit().await?;
            return Ok(value);
        }
        let current = load_installation(&transaction, &command.plugin_id).await?;
        let target = load_version(&transaction, &command.target_version_id).await?;
        if current.active_version.id != command.expected_active_version_id
            || target.plugin_id != command.plugin_id
        {
            return Err(StorageError::Conflict("plugin_active_version"));
        }
        let now = now_ms();
        insert_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &command.plugin_id,
            now,
        )
        .await?;
        transaction.execute_raw(sql(
            "UPDATE plugin_installations SET active_version_id = ?, updated_at = ? WHERE plugin_id = ?",
            vec![target.id.into(), now.into(), command.plugin_id.clone().into()],
        )).await?;
        let view = load_installation(&transaction, &command.plugin_id).await?;
        finish_receipt(&transaction, &scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }
}

async fn replay<C: ConnectionTrait>(
    db: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<PluginInstallationView>> {
    let Some(receipt) = find_receipt(db, scope, key).await? else {
        return Ok(None);
    };
    if receipt.digest != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    Ok(Some(
        load_object_json(db, &result_id(receipt.result_object_id)?).await?,
    ))
}
