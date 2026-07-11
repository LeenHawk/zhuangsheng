pub mod channel;
pub mod context;
pub mod error;
pub mod graph;
pub mod memory;
pub mod preset;
pub mod runtime;
pub mod secret;

use std::sync::Arc;

use zhuangsheng_core::application::{
    channel::ChannelService, context::ContextService, graph::GraphService, memory::MemoryService,
    preset::ContextPresetService, secret::SecretStoreService,
};
use zhuangsheng_core::runtime::RuntimeService;

#[derive(Clone)]
pub struct AppState {
    pub graph_service: Arc<dyn GraphService>,
    pub channel_service: Arc<dyn ChannelService>,
    pub preset_service: Arc<dyn ContextPresetService>,
    pub context_service: Arc<dyn ContextService>,
    pub memory_service: Arc<dyn MemoryService>,
    pub runtime_service: Arc<dyn RuntimeService>,
    pub secret_service: Arc<dyn SecretStoreService>,
}
