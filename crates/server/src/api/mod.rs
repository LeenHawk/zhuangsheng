pub mod context;
pub mod error;
pub mod graph;
pub mod memory;
pub mod runtime;

use std::sync::Arc;

use zhuangsheng_core::application::{
    context::ContextService, graph::GraphService, memory::MemoryService,
};
use zhuangsheng_core::runtime::RuntimeService;

#[derive(Clone)]
pub struct AppState {
    pub graph_service: Arc<dyn GraphService>,
    pub context_service: Arc<dyn ContextService>,
    pub memory_service: Arc<dyn MemoryService>,
    pub runtime_service: Arc<dyn RuntimeService>,
}
