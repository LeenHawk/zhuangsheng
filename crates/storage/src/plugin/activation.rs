use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{application::plugin::*, canonical};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::{
    receipt::{finish_receipt, insert_receipt, require_key, result_id},
    rows::{load_candidate, load_installation, policy_name},
};

impl SqliteStore {
    pub async fn activate_plugin_candidate(
        &self,
        mut command: ActivatePluginCandidateCommand,
    ) -> StorageResult<PluginInstallationView> {
        require_key(&command.idempotency_key)?;
        command.approved_permissions.sort();
        command.approved_permissions.dedup();
        let candidate = load_candidate(&self.db, &command.candidate_id).await?;
        let scope = format!("workspace:local:plugins:{}:activate", candidate.manifest.id);
        let digest = canonical::hash(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, &scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let result =
                load_object_json(&transaction, &result_id(receipt.result_object_id)?).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        if command.approved_permissions != candidate.manifest.permissions {
            return Err(StorageError::InvalidArgument(
                "all plugin permissions must be explicitly approved".into(),
            ));
        }
        let current = load_optional_installation(&transaction, &candidate.manifest.id).await?;
        let current_id = current.as_ref().map(|item| item.active_version.id.clone());
        if current_id != command.expected_active_version_id
            || candidate.current_version_id != command.expected_active_version_id
        {
            return Err(StorageError::Conflict("plugin_active_version"));
        }
        require_dependencies(&transaction, &candidate).await?;
        if current
            .as_ref()
            .is_some_and(|item| item.source_url != candidate.source_url)
        {
            return Err(StorageError::Conflict("plugin_source_changed"));
        }
        let now = now_ms();
        insert_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &candidate.manifest.id,
            now,
        )
        .await?;
        if current.is_none() {
            transaction.execute_raw(sql(
                "INSERT INTO plugin_installations (plugin_id, source_url, source_ref, credential_secret_id, credential_username, update_policy, enabled, active_version_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
                vec![candidate.manifest.id.clone().into(), candidate.source_url.clone().into(), candidate.source_ref.clone().into(),
                    candidate.credential_secret_id.clone().into(), candidate.credential_username.clone().into(), policy_name(command.update_policy).into(),
                    candidate.planned_version_id.clone().into(), now.into(), now.into()],
            )).await?;
        }
        insert_version(&transaction, &candidate, now).await?;
        if current.is_some() {
            transaction.execute_raw(sql(
                "UPDATE plugin_installations SET source_ref = ?, credential_secret_id = ?, credential_username = ?, update_policy = ?, active_version_id = ?, updated_at = ? WHERE plugin_id = ?",
                vec![candidate.source_ref.clone().into(), candidate.credential_secret_id.clone().into(), candidate.credential_username.clone().into(),
                    policy_name(command.update_policy).into(), candidate.planned_version_id.clone().into(), now.into(), candidate.manifest.id.clone().into()],
            )).await?;
        }
        transaction.execute_raw(sql(
            "UPDATE plugin_candidates SET status = 'activated', activated_version_id = ? WHERE id = ? AND status = 'staged'",
            vec![candidate.planned_version_id.clone().into(), candidate.id.clone().into()],
        )).await?;
        let view = load_installation(&transaction, &candidate.manifest.id).await?;
        finish_receipt(&transaction, &scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }
}

async fn insert_version<C: ConnectionTrait>(
    db: &C,
    candidate: &PluginCandidateView,
    now: i64,
) -> StorageResult<()> {
    if db
        .query_one_raw(sql(
            "SELECT 1 AS present FROM plugin_versions WHERE plugin_id = ? AND resolved_commit = ?",
            vec![
                candidate.manifest.id.clone().into(),
                candidate.resolved_commit.clone().into(),
            ],
        ))
        .await?
        .is_some()
    {
        return Err(StorageError::Conflict("plugin_commit_installed"));
    }
    db.execute_raw(sql(
        "INSERT INTO plugin_versions (id, plugin_id, version, resolved_commit, tree_hash, manifest_hash, manifest_json, installed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        vec![candidate.planned_version_id.clone().into(), candidate.manifest.id.clone().into(), candidate.manifest.version.clone().into(),
            candidate.resolved_commit.clone().into(), candidate.tree_hash.clone().into(), candidate.manifest_hash.clone().into(),
            canonical::to_string(&candidate.manifest)?.into(), now.into()],
    )).await?;
    Ok(())
}

async fn load_optional_installation<C: ConnectionTrait>(
    db: &C,
    plugin_id: &str,
) -> StorageResult<Option<PluginInstallationView>> {
    match load_installation(db, plugin_id).await {
        Ok(value) => Ok(Some(value)),
        Err(StorageError::NotFound { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

async fn require_dependencies<C: ConnectionTrait>(
    db: &C,
    candidate: &PluginCandidateView,
) -> StorageResult<()> {
    for dependency in &candidate.manifest.dependencies {
        let installed = load_optional_installation(db, dependency).await?;
        if !installed.is_some_and(|item| item.enabled) {
            return Err(StorageError::Conflict("plugin_dependency_missing"));
        }
    }
    Ok(())
}
