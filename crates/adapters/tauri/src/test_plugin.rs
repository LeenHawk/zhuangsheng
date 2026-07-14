use std::sync::Arc;

use async_trait::async_trait;
use zhuangsheng_core::application::{ApplicationError, plugin::*};

pub(super) fn service() -> Arc<dyn PluginPackageService> {
    Arc::new(TestPluginService)
}

struct TestPluginService;

#[async_trait]
impl PluginPackageService for TestPluginService {
    async fn inspect_git_source(
        &self,
        _: InspectGitPluginCommand,
    ) -> Result<PluginCandidateView, ApplicationError> {
        Err(ApplicationError::Unavailable)
    }

    async fn activate_candidate(
        &self,
        _: ActivatePluginCandidateCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        Err(ApplicationError::Unavailable)
    }

    async fn list_installations(&self) -> Result<Vec<PluginInstallationView>, ApplicationError> {
        Ok(Vec::new())
    }

    async fn configure_plugin(
        &self,
        _: ConfigurePluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        Err(ApplicationError::Unavailable)
    }

    async fn check_update(&self, _: &str) -> Result<Option<PluginCandidateView>, ApplicationError> {
        Err(ApplicationError::Unavailable)
    }

    async fn rollback_plugin(
        &self,
        _: RollbackPluginCommand,
    ) -> Result<PluginInstallationView, ApplicationError> {
        Err(ApplicationError::Unavailable)
    }

    async fn get_entrypoint(&self, _: &str) -> Result<PluginEntrypointView, ApplicationError> {
        Err(ApplicationError::Unavailable)
    }

    async fn refresh_automatic(&self) -> Result<Vec<PluginInstallationView>, ApplicationError> {
        Ok(Vec::new())
    }
}
