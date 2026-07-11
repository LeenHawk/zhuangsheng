mod api;

use std::sync::Arc;

use axum::{Router, extract::DefaultBodyLimit, routing::get};
use serde::Serialize;
use tower_http::trace::TraceLayer;
use zhuangsheng_core::application::{
    context::ContextService, graph::GraphService, memory::MemoryService,
};
use zhuangsheng_core::runtime::RuntimeService;

use api::AppState;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

pub fn app(
    graph_service: Arc<dyn GraphService>,
    context_service: Arc<dyn ContextService>,
    memory_service: Arc<dyn MemoryService>,
    runtime_service: Arc<dyn RuntimeService>,
) -> Router {
    let state = AppState {
        graph_service,
        context_service,
        memory_service,
        runtime_service,
    };
    Router::new()
        .route(
            "/health",
            get(|| async { axum::Json(Health { status: "ok" }) }),
        )
        .merge(api::graph::routes())
        .merge(api::context::routes())
        .merge(api::memory::routes())
        .merge(api::runtime::routes())
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[cfg(test)]
mod tests;
