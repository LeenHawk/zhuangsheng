use std::sync::Arc;

use zhuangsheng_core::application::{
    channel::ChannelService, conversation::ConversationService, graph::GraphService,
    preset::ContextPresetService, secret::SecretStoreService,
};
use zhuangsheng_core::runtime::RuntimeService;

mod bootstrap;
mod error;
mod runtime;
mod secret;

pub use error::{CommandResult, TauriCommandError};
pub use secret::{SensitivePasswordInput, SensitiveSecretInput};

pub struct TauriAdapter {
    pub(crate) runtime: Arc<dyn RuntimeService>,
    pub(crate) graph: Arc<dyn GraphService>,
    pub(crate) channel: Arc<dyn ChannelService>,
    pub(crate) preset: Arc<dyn ContextPresetService>,
    pub(crate) conversation: Arc<dyn ConversationService>,
    pub(crate) secret: Arc<dyn SecretStoreService>,
}

impl TauriAdapter {
    pub fn new(
        runtime: Arc<dyn RuntimeService>,
        graph: Arc<dyn GraphService>,
        channel: Arc<dyn ChannelService>,
        preset: Arc<dyn ContextPresetService>,
        conversation: Arc<dyn ConversationService>,
        secret: Arc<dyn SecretStoreService>,
    ) -> Self {
        Self {
            runtime,
            graph,
            channel,
            preset,
            conversation,
            secret,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zhuangsheng_core::application::graph::CreateGraphCommand;
    use zhuangsheng_storage::SqliteStore;

    use super::{SensitiveSecretInput, TauriAdapter};

    #[tokio::test]
    async fn adapter_calls_the_same_application_services_without_protocol_state() {
        let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
        let adapter = TauriAdapter::new(
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
        );
        let created = adapter
            .create_graph(CreateGraphCommand {
                name: "Local graph".into(),
                idempotency_key: "tauri-create-graph".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            store
                .get_graph_draft(&created.graph.id)
                .await
                .unwrap()
                .graph_id,
            created.graph.id
        );

        let input: SensitiveSecretInput = serde_json::from_value(serde_json::json!({
            "masterPassword":"correct horse battery staple",
            "idempotencyKey":"tauri-secret-init"
        }))
        .unwrap();
        let session = adapter.initialize_secret_store(input).await.unwrap();
        assert!(!session.session_id.is_empty());
        assert!(!adapter.get_secret_store_status().await.unwrap().locked);
    }
}
