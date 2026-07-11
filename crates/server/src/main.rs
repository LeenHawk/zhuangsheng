use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let app = Router::new().route("/health", get(|| async { Json(Health { status: "ok" }) }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!(address = %listener.local_addr()?, "server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
