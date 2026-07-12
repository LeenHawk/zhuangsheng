#[cfg(feature = "tauri-runtime")]
use std::{fs, sync::Arc, time::Duration};

#[cfg(feature = "tauri-runtime")]
use tauri::{Emitter, Manager};
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_core::scheduler::{Scheduler, SchedulerStore};
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_server::{
    RemoteModelDiscoveryService, llm_executor::LocalLlmExecutor, provider::HttpProviderClient,
};
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_storage::SqliteStore;
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_tauri_adapter::{TauriAdapter, TauriServices};

#[cfg(feature = "tauri-ipc")]
mod commands;

#[cfg(feature = "tauri-runtime")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            fs::create_dir_all(&directory)?;
            let database_url = format!(
                "sqlite://{}?mode=rwc",
                directory.join("zhuangsheng.db").display()
            );
            let store = Arc::new(tauri::async_runtime::block_on(SqliteStore::connect(
                database_url,
            ))?);
            tauri::async_runtime::block_on(store.recover_runtime_runs())?;
            let llm_executor = Arc::new(LocalLlmExecutor::new(store.clone())?);
            let scheduler_store: Arc<dyn SchedulerStore> = store.clone();
            tauri::async_runtime::spawn(async move {
                let scheduler = Scheduler::new(scheduler_store, "tauri-local-worker")
                    .with_llm_executor(llm_executor);
                loop {
                    let worked = scheduler.run_one(now_ms()).await.unwrap_or(false);
                    if !worked {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
            });
            let projection_store = store.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let _ = projection_store
                        .maintain_candidate_projections(
                            now_ms(),
                            "tauri-conversation-projector",
                            50,
                        )
                        .await;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            });
            let model_discovery = Arc::new(RemoteModelDiscoveryService::new(
                store.clone(),
                store.clone(),
                Arc::new(HttpProviderClient::new()?),
            ));
            app.manage(TauriAdapter::new(TauriServices {
                runtime: store.clone(),
                graph: store.clone(),
                channel: store.clone(),
                model_discovery: Some(model_discovery),
                preset: store.clone(),
                conversation: store.clone(),
                context: store.clone(),
                memory: store.clone(),
                secret: store.clone(),
            }));
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let _ = handle.emit("zhuangsheng://run-events", ());
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_run,
            commands::get_run,
            commands::list_recent_runs,
            commands::list_open_waits,
            commands::list_run_events,
            commands::interrupt_run,
            commands::resume_run,
            commands::cancel_run,
            commands::satisfy_wait,
            commands::resolve_effect_unknown,
            commands::fork_context,
            commands::merge_context,
            commands::create_graph,
            commands::create_channel,
            commands::create_context_preset,
            commands::create_conversation,
            commands::get_roleplay_settings,
            commands::update_conversation_run_profile,
            commands::get_secret_store_status,
            commands::initialize_secret_store,
            commands::unlock_secret_store,
            commands::list_secrets,
            commands::put_secret,
            commands::conversation::list_conversations,
            commands::conversation::get_conversation,
            commands::conversation::get_conversation_timeline,
            commands::conversation::get_turn_candidates,
            commands::conversation::submit_conversation_turn,
            commands::conversation::select_conversation_candidate,
            commands::conversation::regenerate_conversation_candidate,
            commands::conversation::resolve_candidate_projection,
            commands::conversation::list_roleplay_graph_options,
            commands::conversation::get_roleplay_compatibility,
            commands::conversation::list_context_branches,
            commands::config::list_channels,
            commands::config::publish_channel_revision,
            commands::config::get_channel_revision,
            commands::config::discover_channel_models,
            commands::config::list_context_presets,
            commands::config::publish_context_preset_version,
            commands::config::get_context_preset_version,
            commands::config::preview_context_preset,
            commands::config::create_roleplay_template,
            commands::config::get_graph_revision,
            commands::memory::list_memory_proposals,
            commands::memory::propose_memory_change,
            commands::memory::decide_memory_proposal,
            commands::memory::apply_memory_proposal,
            commands::memory::get_memory_record,
            commands::memory::search_memory,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run zhuangsheng desktop application");
}

#[cfg(not(feature = "tauri-runtime"))]
pub fn run() {
    panic!("build with --features tauri-runtime to launch the Tauri shell");
}

#[cfg(feature = "tauri-runtime")]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}
