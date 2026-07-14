use zhuangsheng_core::application::plugin::*;

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn inspect_git_plugin_source(
        &self,
        command: InspectGitPluginCommand,
    ) -> CommandResult<PluginCandidateView> {
        Ok(self.plugin.inspect_git_source(command).await?)
    }

    pub async fn activate_plugin_candidate(
        &self,
        command: ActivatePluginCandidateCommand,
    ) -> CommandResult<PluginInstallationView> {
        Ok(self.plugin.activate_candidate(command).await?)
    }

    pub async fn list_plugins(&self) -> CommandResult<Vec<PluginInstallationView>> {
        Ok(self.plugin.list_installations().await?)
    }

    pub async fn configure_plugin(
        &self,
        command: ConfigurePluginCommand,
    ) -> CommandResult<PluginInstallationView> {
        Ok(self.plugin.configure_plugin(command).await?)
    }

    pub async fn check_plugin_update(
        &self,
        plugin_id: &str,
    ) -> CommandResult<Option<PluginCandidateView>> {
        Ok(self.plugin.check_update(plugin_id).await?)
    }

    pub async fn rollback_plugin(
        &self,
        command: RollbackPluginCommand,
    ) -> CommandResult<PluginInstallationView> {
        Ok(self.plugin.rollback_plugin(command).await?)
    }

    pub async fn get_plugin_entrypoint(
        &self,
        plugin_id: &str,
    ) -> CommandResult<PluginEntrypointView> {
        Ok(self.plugin.get_entrypoint(plugin_id).await?)
    }
}
