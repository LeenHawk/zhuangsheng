use std::sync::Arc;

use zhuangsheng_core::application::{
    channel::{ChannelModelDiscoveryService, ChannelService},
    context::ContextService,
    conversation::ConversationService,
    graph::GraphService,
    memory::MemoryService,
    preset::ContextPresetService,
    secret::SecretStoreService,
};
use zhuangsheng_core::runtime::RuntimeService;

mod bootstrap;
mod config;
mod conversation;
mod error;
mod memory;
mod runtime;
mod secret;

pub use error::{CommandResult, TauriCommandError};
pub use memory::{DecideMemoryProposalInput, ProposeMemoryChangeInput};
pub use runtime::{ResolveEffectUnknownInput, SatisfyWaitInput};
pub use secret::{SensitivePasswordInput, SensitivePutSecretInput, SensitiveSecretInput};

pub struct TauriServices {
    pub runtime: Arc<dyn RuntimeService>,
    pub graph: Arc<dyn GraphService>,
    pub channel: Arc<dyn ChannelService>,
    pub model_discovery: Option<Arc<dyn ChannelModelDiscoveryService>>,
    pub preset: Arc<dyn ContextPresetService>,
    pub conversation: Arc<dyn ConversationService>,
    pub context: Arc<dyn ContextService>,
    pub memory: Arc<dyn MemoryService>,
    pub secret: Arc<dyn SecretStoreService>,
}

pub struct TauriAdapter {
    pub(crate) runtime: Arc<dyn RuntimeService>,
    pub(crate) graph: Arc<dyn GraphService>,
    pub(crate) channel: Arc<dyn ChannelService>,
    pub(crate) model_discovery: Option<Arc<dyn ChannelModelDiscoveryService>>,
    pub(crate) preset: Arc<dyn ContextPresetService>,
    pub(crate) conversation: Arc<dyn ConversationService>,
    pub(crate) context: Arc<dyn ContextService>,
    pub(crate) memory: Arc<dyn MemoryService>,
    pub(crate) secret: Arc<dyn SecretStoreService>,
}

impl TauriAdapter {
    pub fn new(services: TauriServices) -> Self {
        Self {
            runtime: services.runtime,
            graph: services.graph,
            channel: services.channel,
            model_discovery: services.model_discovery,
            preset: services.preset,
            conversation: services.conversation,
            context: services.context,
            memory: services.memory,
            secret: services.secret,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zhuangsheng_core::application::graph::CreateGraphCommand;
    use zhuangsheng_storage::SqliteStore;

    use super::{SensitiveSecretInput, TauriAdapter, TauriServices};

    #[tokio::test]
    async fn adapter_calls_the_same_application_services_without_protocol_state() {
        let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
        let adapter = TauriAdapter::new(TauriServices {
            runtime: store.clone(),
            graph: store.clone(),
            channel: store.clone(),
            model_discovery: None,
            preset: store.clone(),
            conversation: store.clone(),
            context: store.clone(),
            memory: store.clone(),
            secret: store.clone(),
        });
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
