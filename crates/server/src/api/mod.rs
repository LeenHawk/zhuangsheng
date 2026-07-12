pub mod artifact;
mod artifact_upload;
pub mod channel;
pub mod context;
pub mod context_merge;
pub mod conversation;
pub mod conversation_candidate;
pub mod error;
pub mod graph;
pub mod memory;
pub mod preset;
pub mod runtime;
pub mod secret;
pub mod tool;

use std::sync::Arc;

use zhuangsheng_core::application::{
    artifact::ArtifactStagingService, channel::ChannelService, context::ContextService,
    conversation::ConversationService, graph::GraphService, memory::MemoryService,
    preset::ContextPresetService, secret::SecretStoreService,
};
use zhuangsheng_core::runtime::RuntimeService;

use crate::StreamEventHub;

#[derive(Clone)]
pub struct AppState {
    pub artifact_service: Arc<dyn ArtifactStagingService>,
    pub graph_service: Arc<dyn GraphService>,
    pub channel_service: Arc<dyn ChannelService>,
    pub preset_service: Arc<dyn ContextPresetService>,
    pub context_service: Arc<dyn ContextService>,
    pub conversation_service: Arc<dyn ConversationService>,
    pub memory_service: Arc<dyn MemoryService>,
    pub runtime_service: Arc<dyn RuntimeService>,
    pub secret_service: Arc<dyn SecretStoreService>,
    pub tool_registry_service: Arc<dyn zhuangsheng_core::application::tool::ToolRegistryService>,
    pub stream_events: StreamEventHub,
}
