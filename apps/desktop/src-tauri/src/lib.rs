#[cfg(feature = "tauri-runtime")]
use std::{fs, sync::Arc, time::Duration};

#[cfg(feature = "tauri-runtime")]
use tauri::{Emitter, Manager};
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_core::scheduler::{Scheduler, SchedulerStore};
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_storage::SqliteStore;
#[cfg(feature = "tauri-runtime")]
use zhuangsheng_tauri_adapter::TauriAdapter;

#[cfg(feature = "tauri-runtime")]
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
            let scheduler_store: Arc<dyn SchedulerStore> = store.clone();
            tauri::async_runtime::spawn(async move {
                let scheduler = Scheduler::new(scheduler_store, "tauri-local-worker");
                loop {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |duration| {
                            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
                        });
                    let worked = scheduler.run_one(now).await.unwrap_or(false);
                    if !worked {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
            });
            app.manage(TauriAdapter::new(
                store.clone(),
                store.clone(),
                store.clone(),
                store.clone(),
                store.clone(),
                store.clone(),
            ));
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
        ])
        .run(tauri::generate_context!())
        .expect("failed to run zhuangsheng desktop application");
}

#[cfg(not(feature = "tauri-runtime"))]
pub fn run() {
    panic!("build with --features tauri-runtime to launch the Tauri shell");
}
