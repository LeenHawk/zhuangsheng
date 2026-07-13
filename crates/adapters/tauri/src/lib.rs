use std::sync::Arc;

use zhuangsheng_core::application::{
    artifact::ArtifactStagingService,
    channel::{ChannelModelDiscoveryService, ChannelService},
    context::ContextService,
    conversation::ConversationService,
    graph::GraphService,
    memory::MemoryService,
    preset::ContextPresetService,
    secret::SecretStoreService,
    tool::ToolRegistryService,
};
use zhuangsheng_core::runtime::RuntimeService;

mod artifact;
mod bootstrap;
mod config;
mod context;
mod conversation;
mod error;
mod exact;
mod memory;
mod runtime;
mod secret;
mod tool;

pub use artifact::{ArtifactDownloadView, CompleteArtifactStagingInput};
pub use error::{CommandResult, TauriCommandError};
pub use memory::{DecideMemoryProposalInput, ProposeMemoryChangeInput};
pub use runtime::{ResolveEffectUnknownInput, SatisfyWaitInput};
pub use secret::SensitiveChangePasswordInput;
pub use secret::{SensitivePasswordInput, SensitivePutSecretInput, SensitiveSecretInput};

pub struct TauriServices {
    pub runtime: Arc<dyn RuntimeService>,
    pub artifact: Arc<dyn ArtifactStagingService>,
    pub graph: Arc<dyn GraphService>,
    pub channel: Arc<dyn ChannelService>,
    pub model_discovery: Option<Arc<dyn ChannelModelDiscoveryService>>,
    pub preset: Arc<dyn ContextPresetService>,
    pub conversation: Arc<dyn ConversationService>,
    pub context: Arc<dyn ContextService>,
    pub memory: Arc<dyn MemoryService>,
    pub secret: Arc<dyn SecretStoreService>,
    pub tool_registry: Arc<dyn ToolRegistryService>,
}

pub struct TauriAdapter {
    pub(crate) runtime: Arc<dyn RuntimeService>,
    pub(crate) artifact: Arc<dyn ArtifactStagingService>,
    pub(crate) graph: Arc<dyn GraphService>,
    pub(crate) channel: Arc<dyn ChannelService>,
    pub(crate) model_discovery: Option<Arc<dyn ChannelModelDiscoveryService>>,
    pub(crate) preset: Arc<dyn ContextPresetService>,
    pub(crate) conversation: Arc<dyn ConversationService>,
    pub(crate) context: Arc<dyn ContextService>,
    pub(crate) memory: Arc<dyn MemoryService>,
    pub(crate) secret: Arc<dyn SecretStoreService>,
    pub(crate) tool_registry: Arc<dyn ToolRegistryService>,
}

impl TauriAdapter {
    pub fn new(services: TauriServices) -> Self {
        Self {
            runtime: services.runtime,
            artifact: services.artifact,
            graph: services.graph,
            channel: services.channel,
            model_discovery: services.model_discovery,
            preset: services.preset,
            conversation: services.conversation,
            context: services.context,
            memory: services.memory,
            secret: services.secret,
            tool_registry: services.tool_registry,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zhuangsheng_core::{
        application::{
            context::CommitContextPatchCommand, conversation::CreateConversationCommand,
            graph::CreateGraphCommand,
        },
        state::{ActorKind, ActorRef, JsonPatchOp, StatePatch},
    };
    use zhuangsheng_storage::SqliteStore;

    use super::{SensitiveSecretInput, TauriAdapter, TauriServices};

    #[tokio::test]
    async fn adapter_calls_the_same_application_services_without_protocol_state() {
        let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
        let adapter = TauriAdapter::new(TauriServices {
            runtime: store.clone(),
            artifact: store.clone(),
            graph: store.clone(),
            channel: store.clone(),
            model_discovery: None,
            preset: store.clone(),
            conversation: store.clone(),
            context: store.clone(),
            memory: store.clone(),
            secret: store.clone(),
            tool_registry: store.clone(),
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

        let conversation = adapter
            .create_conversation(CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "tauri-conversation".into(),
            })
            .await
            .unwrap();
        let commit = adapter
            .commit_context_patch(CommitContextPatchCommand {
                patch: StatePatch {
                    aggregate_kind: zhuangsheng_core::state::AggregateKind::WorkingContext,
                    aggregate_id: conversation.context_id,
                    lineage_key: conversation.active_branch_id,
                    base_commit_id: conversation.active_head_commit_id,
                    operation_id: "tauri-context-patch".into(),
                    ops: vec![JsonPatchOp::Add {
                        path: "/local".into(),
                        value: serde_json::json!(true),
                    }],
                    schema_version: 1,
                    policy_version: 1,
                    author: ActorRef {
                        kind: ActorKind::Node,
                        id: Some("forged-node".into()),
                    },
                },
                origin_run_id: None,
                origin_node_instance_id: None,
            })
            .await
            .unwrap();
        assert_eq!(commit.author.kind, ActorKind::User);
        assert_eq!(commit.author.id.as_deref(), Some("local-user"));
    }
}
