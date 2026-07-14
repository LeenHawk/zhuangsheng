use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::plugin::{
        PluginCandidateView, PluginInstallationView, RegisterPluginCandidateCommand,
        normalize_plugin_manifest,
    },
    canonical,
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::{
    candidate_lookup::load_staged_candidate_by_commit,
    rows::{list_installations, load_candidate, load_installation},
};

impl SqliteStore {
    pub async fn register_plugin_candidate(
        &self,
        command: RegisterPluginCandidateCommand,
    ) -> StorageResult<PluginCandidateView> {
        let mut candidate = command.candidate;
        candidate.manifest = normalize_plugin_manifest(candidate.manifest)
            .map_err(|_| StorageError::InvalidArgument("plugin manifest is invalid".into()))?;
        if candidate.id.is_empty()
            || candidate.planned_version_id.is_empty()
            || candidate.manifest.id.is_empty()
            || candidate.resolved_commit.len() != 40
        {
            return Err(StorageError::InvalidArgument(
                "plugin candidate identity is invalid".into(),
            ));
        }
        candidate.added_permissions.sort();
        candidate.added_permissions.dedup();
        let manifest_json = canonical::to_string(&candidate.manifest)?;
        let manifest_hash = canonical::hash(&candidate.manifest)?;
        if candidate.manifest_hash != manifest_hash {
            return Err(StorageError::InvalidArgument(
                "plugin candidate manifest hash mismatch".into(),
            ));
        }
        if let Some(existing) = load_staged_candidate_by_commit(
            &self.db,
            &candidate.manifest.id,
            &candidate.resolved_commit,
        )
        .await?
        {
            return Ok(existing);
        }
        match load_candidate(&self.db, &candidate.id).await {
            Ok(existing) if existing == candidate => return Ok(existing),
            Ok(_) => return Err(StorageError::Conflict("plugin_candidate_exists")),
            Err(StorageError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }
        self.db.execute_raw(sql(
            "INSERT INTO plugin_candidates (id, planned_version_id, plugin_id, source_url, source_ref, credential_secret_id, credential_username, resolved_commit, tree_hash, manifest_hash, manifest_json, current_version_id, added_permissions_json, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'staged', ?)",
            vec![
                candidate.id.clone().into(), candidate.planned_version_id.clone().into(), candidate.manifest.id.clone().into(),
                candidate.source_url.clone().into(), candidate.source_ref.clone().into(), candidate.credential_secret_id.clone().into(),
                candidate.credential_username.clone().into(), candidate.resolved_commit.clone().into(), candidate.tree_hash.clone().into(),
                manifest_hash.into(), manifest_json.into(), candidate.current_version_id.clone().into(),
                canonical::to_string(&candidate.added_permissions)?.into(), candidate.created_at.into(),
            ],
        )).await?;
        load_candidate(&self.db, &candidate.id).await
    }

    pub async fn get_plugin_candidate(
        &self,
        candidate_id: &str,
    ) -> StorageResult<PluginCandidateView> {
        load_candidate(&self.db, candidate_id).await
    }

    pub async fn list_plugin_installations(&self) -> StorageResult<Vec<PluginInstallationView>> {
        list_installations(&self.db).await
    }

    pub async fn get_plugin_installation(
        &self,
        plugin_id: &str,
    ) -> StorageResult<PluginInstallationView> {
        load_installation(&self.db, plugin_id).await
    }
}
