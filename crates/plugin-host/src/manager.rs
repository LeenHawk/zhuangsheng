use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use tokio::fs;
use zhuangsheng_core::{
    application::{ApplicationError, plugin::*, secret::SecretResolver},
    canonical,
};

use crate::{
    package::inspect_package,
    source::{candidate_root, invalid, version_root},
    staging::{StageResult, stage_source},
    update::refresh_automatic,
};

pub struct GitPluginManager {
    pub(super) registry: Arc<dyn PluginRegistryStore>,
    pub(super) secrets: Arc<dyn SecretResolver>,
    pub(super) root: PathBuf,
}

impl GitPluginManager {
    pub fn new(
        registry: Arc<dyn PluginRegistryStore>,
        secrets: Arc<dyn SecretResolver>,
        root: impl Into<PathBuf>,
    ) -> Result<Self, std::io::Error> {
        let root = root.into();
        std::fs::create_dir_all(root.join("staging"))?;
        std::fs::create_dir_all(root.join("versions"))?;
        Ok(Self {
            registry,
            secrets,
            root,
        })
    }

    async fn activate_files(
        &self,
        candidate: &PluginCandidateView,
    ) -> Result<PathBuf, ApplicationError> {
        let candidate_dir = candidate_root(&self.root, &candidate.id);
        let work = candidate_dir.join("work");
        let destination = version_root(
            &self.root,
            &candidate.manifest.id,
            &candidate.planned_version_id,
        );
        if !destination.exists() {
            fs::create_dir_all(destination.parent().ok_or(ApplicationError::Internal)?)
                .await
                .map_err(|_| ApplicationError::Internal)?;
            fs::rename(&work, &destination)
                .await
                .map_err(|_| ApplicationError::Internal)?;
        }
        let verify = destination.clone();
        let inspected = tokio::task::spawn_blocking(move || inspect_package(&verify))
            .await
            .map_err(|_| ApplicationError::Internal)??;
        if inspected.manifest_hash != candidate.manifest_hash
            || inspected.tree_hash != candidate.tree_hash
            || inspected.manifest != candidate.manifest
        {
            return Err(invalid(
                "plugin_candidate_changed",
                "staged plugin files changed before activation",
            ));
        }
        Ok(candidate_dir)
    }

    async fn load_entrypoint(
        &self,
        installation: &PluginInstallationView,
    ) -> Result<PluginEntrypointView, ApplicationError> {
        if !installation.enabled {
            return Err(ApplicationError::Conflict("plugin_disabled"));
        }
        let version = &installation.active_version;
        let root = version_root(&self.root, &installation.plugin_id, &version.id);
        let verify = root.clone();
        let inspected = tokio::task::spawn_blocking(move || inspect_package(&verify))
            .await
            .map_err(|_| ApplicationError::Internal)??;
        if inspected.tree_hash != version.tree_hash
            || inspected.manifest_hash != version.manifest_hash
        {
            return Err(ApplicationError::Internal);
        }
        let bytes = fs::read(root.join(&version.manifest.entrypoints.ui_worker))
            .await
            .map_err(|_| ApplicationError::Internal)?;
        let code = String::from_utf8(bytes.clone()).map_err(|_| {
            invalid(
                "plugin_entrypoint_encoding",
                "plugin UI worker must be UTF-8",
            )
        })?;
        Ok(PluginEntrypointView {
            plugin_id: installation.plugin_id.clone(),
            version_id: version.id.clone(),
            content_hash: canonical::hash_bytes(&bytes),
            code,
        })
    }
}

#[async_trait]
impl PluginPackageService for GitPluginManager {
    async fn inspect_git_source(
        &self,
        command: InspectGitPluginCommand,
    ) -> Result<PluginCandidateView, ApplicationError> {
        match stage_source(self, command).await? {
            StageResult::Candidate(value) => Ok(value),
            StageResult::UpToDate => Err(invalid(
                "plugin_up_to_date",
                "plugin already uses this Git commit",
            )),
        }
    }

    async fn activate_candidate(
        &self,
        command: ActivatePluginCandidateCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        let candidate = self
            .registry
            .get_plugin_candidate(&command.candidate_id)
            .await?;
        let candidate_dir = self.activate_files(&candidate).await?;
        let result = self.registry.activate_plugin_candidate(command).await;
        if result.is_ok() {
            let _ = fs::remove_dir_all(candidate_dir).await;
        }
        result
    }

    async fn list_installations(&self) -> Result<Vec<PluginInstallationView>, ApplicationError> {
        self.registry.list_plugin_installations().await
    }

    async fn configure_plugin(
        &self,
        command: ConfigurePluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        self.registry.configure_plugin_installation(command).await
    }

    async fn check_update(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCandidateView>, ApplicationError> {
        let installed = self.registry.get_plugin_installation(plugin_id).await?;
        let command = InspectGitPluginCommand {
            source_url: installed.source_url,
            source_ref: installed.source_ref,
            credential_secret_id: installed.credential_secret_id,
            credential_username: installed.credential_username,
        };
        match stage_source(self, command).await? {
            StageResult::Candidate(value) => Ok(Some(value)),
            StageResult::UpToDate => Ok(None),
        }
    }

    async fn rollback_plugin(
        &self,
        command: RollbackPluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        self.registry.rollback_plugin_installation(command).await
    }

    async fn get_entrypoint(
        &self,
        plugin_id: &str,
    ) -> Result<PluginEntrypointView, ApplicationError> {
        let installed = self.registry.get_plugin_installation(plugin_id).await?;
        self.load_entrypoint(&installed).await
    }

    async fn refresh_automatic(&self) -> Result<Vec<PluginInstallationView>, ApplicationError> {
        refresh_automatic(self).await
    }
}
