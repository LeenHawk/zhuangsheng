#[cfg(feature = "http")]
mod api;
pub mod llm_executor;
mod llm_executor_support;
mod model_discovery;
pub mod provider;
mod stream_events;
mod tool_executor;

pub use model_discovery::RemoteModelDiscoveryService;
pub use stream_events::{EphemeralLlmStreamEvent, StreamEventHub};

#[cfg(feature = "http")]
use std::sync::Arc;

#[cfg(feature = "http")]
use axum::{Router, extract::DefaultBodyLimit, routing::get};
#[cfg(feature = "http")]
use serde::Serialize;
#[cfg(feature = "http")]
use tower_http::trace::TraceLayer;
#[cfg(feature = "http")]
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
#[cfg(feature = "http")]
use zhuangsheng_core::runtime::RuntimeService;

#[cfg(feature = "http")]
use api::AppState;

#[cfg(feature = "http")]
#[derive(Serialize)]
struct Health {
    status: &'static str,
}

#[cfg(feature = "http")]
pub struct AppServices {
    pub artifact: Arc<dyn ArtifactStagingService>,
    pub graph: Arc<dyn GraphService>,
    pub channel: Arc<dyn ChannelService>,
    pub model_discovery: Arc<dyn ChannelModelDiscoveryService>,
    pub preset: Arc<dyn ContextPresetService>,
    pub context: Arc<dyn ContextService>,
    pub conversation: Arc<dyn ConversationService>,
    pub memory: Arc<dyn MemoryService>,
    pub runtime: Arc<dyn RuntimeService>,
    pub secret: Arc<dyn SecretStoreService>,
    pub tool_registry: Arc<dyn ToolRegistryService>,
    pub stream_events: StreamEventHub,
}

#[cfg(feature = "http")]
pub fn app(services: AppServices) -> Router {
    let state = AppState {
        artifact_service: services.artifact,
        graph_service: services.graph,
        channel_service: services.channel,
        model_discovery_service: services.model_discovery,
        preset_service: services.preset,
        context_service: services.context,
        conversation_service: services.conversation,
        memory_service: services.memory,
        runtime_service: services.runtime,
        secret_service: services.secret,
        tool_registry_service: services.tool_registry,
        stream_events: services.stream_events,
    };
    Router::new()
        .route(
            "/health",
            get(|| async { axum::Json(Health { status: "ok" }) }),
        )
        .merge(api::graph::routes())
        .merge(api::graph_roleplay::routes())
        .merge(api::artifact::routes())
        .merge(api::channel::routes())
        .merge(api::preset::routes())
        .merge(api::context::routes())
        .merge(api::context_merge::routes())
        .merge(api::effect::routes())
        .merge(api::conversation::routes())
        .merge(api::conversation_candidate::routes())
        .merge(api::memory::routes())
        .merge(api::runtime::routes())
        .merge(api::secret::routes())
        .merge(api::tool::routes())
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod llm_executor_tests;
#[cfg(test)]
mod tests;
