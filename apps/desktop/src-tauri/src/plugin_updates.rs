use std::{path::PathBuf, sync::Arc, time::Duration};

use zhuangsheng_core::application::plugin::PluginPackageService;
use zhuangsheng_plugin_host::GitPluginManager;
use zhuangsheng_storage::SqliteStore;

pub(super) fn create(
    store: Arc<SqliteStore>,
    root: PathBuf,
) -> Result<Arc<dyn PluginPackageService>, std::io::Error> {
    Ok(Arc::new(GitPluginManager::new(
        store.clone(),
        store,
        root,
    )?))
}

pub(super) async fn monitor(service: Arc<dyn PluginPackageService>) {
    loop {
        tokio::time::sleep(Duration::from_secs(15 * 60)).await;
        if let Err(error) = service.refresh_automatic().await {
            tracing::warn!(%error, "automatic plugin update scan failed");
        }
    }
}
