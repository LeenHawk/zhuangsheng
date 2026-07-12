use std::{
    env,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use zhuangsheng_core::{
    application::{
        channel::ChannelService, context::ContextService, graph::GraphService,
        memory::MemoryService, preset::ContextPresetService, secret::SecretStoreService,
        tool::ToolRegistryService,
    },
    runtime::RuntimeService,
    scheduler::{Scheduler, SchedulerStore},
};
use zhuangsheng_server::llm_executor::LocalLlmExecutor;
use zhuangsheng_server::{AppServices, StreamEventHub, app};
use zhuangsheng_storage::SqliteStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zhuangsheng_server=info,tower_http=info".into()),
        )
        .init();
    let database_url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://zhuangsheng.db?mode=rwc".into());
    let bind_address = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let store = Arc::new(SqliteStore::connect(database_url).await?);
    let graph_service: Arc<dyn GraphService> = store.clone();
    let channel_service: Arc<dyn ChannelService> = store.clone();
    let preset_service: Arc<dyn ContextPresetService> = store.clone();
    let context_service: Arc<dyn ContextService> = store.clone();
    let memory_service: Arc<dyn MemoryService> = store.clone();
    let runtime_service: Arc<dyn RuntimeService> = store.clone();
    let secret_service: Arc<dyn SecretStoreService> = store.clone();
    let tool_registry_service: Arc<dyn ToolRegistryService> = store.clone();
    let stream_events = StreamEventHub::new();
    let llm_executor =
        Arc::new(LocalLlmExecutor::new(store.clone())?.with_stream_events(stream_events.clone()));
    let scheduler_store: Arc<dyn SchedulerStore> = store;
    tokio::spawn(run_scheduler(
        Scheduler::new(scheduler_store, "server-local-worker").with_llm_executor(llm_executor),
    ));
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    tracing::info!(address = %listener.local_addr()?, "server listening");
    axum::serve(
        listener,
        app(AppServices {
            graph: graph_service,
            channel: channel_service,
            preset: preset_service,
            context: context_service,
            memory: memory_service,
            runtime: runtime_service,
            secret: secret_service,
            tool_registry: tool_registry_service,
            stream_events,
        }),
    )
    .await?;
    Ok(())
}

async fn run_scheduler(scheduler: Scheduler) {
    loop {
        match scheduler.run_one(now_ms()).await {
            Ok(true) => {}
            Ok(false) => tokio::time::sleep(Duration::from_millis(100)).await,
            Err(error) => {
                tracing::warn!(%error, "scheduler iteration failed");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as i64
}
