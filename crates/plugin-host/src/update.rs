use zhuangsheng_core::application::{ApplicationError, plugin::*};

use crate::manager::GitPluginManager;

pub(super) async fn refresh_automatic(
    manager: &GitPluginManager,
) -> Result<Vec<PluginInstallationView>, ApplicationError> {
    let plugins = manager.list_installations().await?;
    let mut updated = Vec::new();
    for plugin in plugins
        .into_iter()
        .filter(|item| item.enabled && item.update_policy == PluginUpdatePolicy::Automatic)
    {
        let result = update_one(manager, plugin.clone()).await;
        match result {
            Ok(Some(value)) => updated.push(value),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(plugin_id = %plugin.plugin_id, %error, "automatic plugin update failed")
            }
        }
    }
    Ok(updated)
}

async fn update_one(
    manager: &GitPluginManager,
    plugin: PluginInstallationView,
) -> Result<Option<PluginInstallationView>, ApplicationError> {
    let Some(candidate) = manager.check_update(&plugin.plugin_id).await? else {
        return Ok(None);
    };
    if !candidate.added_permissions.is_empty() {
        return Ok(None);
    }
    manager
        .activate_candidate(ActivatePluginCandidateCommand {
            candidate_id: candidate.id,
            expected_active_version_id: Some(plugin.active_version.id),
            approved_permissions: candidate.manifest.permissions.clone(),
            update_policy: PluginUpdatePolicy::Automatic,
            idempotency_key: format!("automatic:{}", candidate.resolved_commit),
        })
        .await
        .map(Some)
}
