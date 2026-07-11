pub mod error;
pub mod graph;
pub mod runtime;

use std::sync::Arc;

use zhuangsheng_core::application::graph::GraphService;
use zhuangsheng_core::runtime::RuntimeService;

#[derive(Clone)]
pub struct AppState {
    pub graph_service: Arc<dyn GraphService>,
    pub runtime_service: Arc<dyn RuntimeService>,
}
