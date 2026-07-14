use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{
    PluginCandidateView, PluginEntrypointView, PluginInstallationView, PluginPermission,
    PluginUpdatePolicy,
};
use crate::application::ApplicationError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectGitPluginCommand {
    pub source_url: String,
    pub source_ref: Option<String>,
    pub credential_secret_id: Option<String>,
    pub credential_username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivatePluginCandidateCommand {
    pub candidate_id: String,
    pub expected_active_version_id: Option<String>,
    pub approved_permissions: Vec<PluginPermission>,
    pub update_policy: PluginUpdatePolicy,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurePluginCommand {
    pub plugin_id: String,
    pub enabled: bool,
    pub update_policy: PluginUpdatePolicy,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPluginCommand {
    pub plugin_id: String,
    pub target_version_id: String,
    pub expected_active_version_id: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPluginCandidateCommand {
    pub candidate: PluginCandidateView,
}

#[async_trait]
pub trait PluginPackageService: Send + Sync {
    async fn inspect_git_source(
        &self,
        command: InspectGitPluginCommand,
    ) -> Result<PluginCandidateView, ApplicationError>;
    async fn activate_candidate(
        &self,
        command: ActivatePluginCandidateCommand,
    ) -> Result<PluginInstallationView, ApplicationError>;
    async fn list_installations(&self) -> Result<Vec<PluginInstallationView>, ApplicationError>;
    async fn configure_plugin(
        &self,
        command: ConfigurePluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError>;
    async fn check_update(
        &self,
        plugin_id: &str,
    ) -> Result<Option<PluginCandidateView>, ApplicationError>;
    async fn rollback_plugin(
        &self,
        command: RollbackPluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError>;
    async fn get_entrypoint(
        &self,
        plugin_id: &str,
    ) -> Result<PluginEntrypointView, ApplicationError>;
    async fn refresh_automatic(&self) -> Result<Vec<PluginInstallationView>, ApplicationError>;
}

#[async_trait]
pub trait PluginRegistryStore: Send + Sync {
    async fn register_plugin_candidate(
        &self,
        command: RegisterPluginCandidateCommand,
    ) -> Result<PluginCandidateView, ApplicationError>;
    async fn get_plugin_candidate(
        &self,
        candidate_id: &str,
    ) -> Result<PluginCandidateView, ApplicationError>;
    async fn activate_plugin_candidate(
        &self,
        command: ActivatePluginCandidateCommand,
    ) -> Result<PluginInstallationView, ApplicationError>;
    async fn list_plugin_installations(
        &self,
    ) -> Result<Vec<PluginInstallationView>, ApplicationError>;
    async fn get_plugin_installation(
        &self,
        plugin_id: &str,
    ) -> Result<PluginInstallationView, ApplicationError>;
    async fn configure_plugin_installation(
        &self,
        command: ConfigurePluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError>;
    async fn rollback_plugin_installation(
        &self,
        command: RollbackPluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError>;
}
