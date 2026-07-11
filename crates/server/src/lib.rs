mod api;

use std::sync::Arc;

use axum::{Router, extract::DefaultBodyLimit, routing::get};
use serde::Serialize;
use tower_http::trace::TraceLayer;
use zhuangsheng_core::application::{
    channel::ChannelService, context::ContextService, graph::GraphService, memory::MemoryService,
    preset::ContextPresetService, secret::SecretStoreService,
};
use zhuangsheng_core::runtime::RuntimeService;

use api::AppState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

pub fn app(
    graph_service: Arc<dyn GraphService>,
    channel_service: Arc<dyn ChannelService>,
    preset_service: Arc<dyn ContextPresetService>,
    context_service: Arc<dyn ContextService>,
    memory_service: Arc<dyn MemoryService>,
    runtime_service: Arc<dyn RuntimeService>,
    secret_service: Arc<dyn SecretStoreService>,
) -> Router {
    let state = AppState {
        graph_service,
        channel_service,
        preset_service,
        context_service,
        memory_service,
        runtime_service,
        secret_service,
    };
    Router::new()
        .route(
            "/health",
            get(|| async { axum::Json(Health { status: "ok" }) }),
        )
        .merge(api::graph::routes())
        .merge(api::channel::routes())
        .merge(api::preset::routes())
        .merge(api::context::routes())
        .merge(api::memory::routes())
        .merge(api::runtime::routes())
        .merge(api::secret::routes())
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests;
