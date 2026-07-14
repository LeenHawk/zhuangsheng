use async_trait::async_trait;
use zhuangsheng_core::application::{ApplicationError, plugin::*};

use crate::SqliteStore;

#[async_trait]
impl PluginRegistryStore for SqliteStore {
    async fn register_plugin_candidate(
        &self,
        command: RegisterPluginCandidateCommand,
    ) -> Result<PluginCandidateView, ApplicationError> {
        SqliteStore::register_plugin_candidate(self, command)
            .await
            .map_err(Into::into)
    }

    async fn get_plugin_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<PluginCandidateView, ApplicationError> {
        SqliteStore::get_plugin_candidate(self, candidate_id)
            .await
            .map_err(Into::into)
    }

    async fn activate_plugin_candidate(
        &self,
        command: ActivatePluginCandidateCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        SqliteStore::activate_plugin_candidate(self, command)
            .await
            .map_err(Into::into)
    }

    async fn list_plugin_installations(
        &self,
    ) -> Result<Vec<PluginInstallationView>, ApplicationError> {
        SqliteStore::list_plugin_installations(self)
            .await
            .map_err(Into::into)
    }

    async fn get_plugin_installation(
        &self,
        plugin_id: &str,
    ) -> Result<PluginInstallationView, ApplicationError> {
        SqliteStore::get_plugin_installation(self, plugin_id)
            .await
            .map_err(Into::into)
    }

    async fn configure_plugin_installation(
        &self,
        command: ConfigurePluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        SqliteStore::configure_plugin_installation(self, command)
            .await
            .map_err(Into::into)
    }

    async fn rollback_plugin_installation(
        &self,
        command: RollbackPluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        SqliteStore::rollback_plugin_installation(self, command)
            .await
            .map_err(Into::into)
    }
}
