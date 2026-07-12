use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Notify;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunOutputValueView, RunStatus, StartRunCommand},
    scheduler::{Scheduler, SchedulerStore},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::{CompletedModelPause, LocalLlmExecutor},
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_llm_graph, now_ms, provider_response};

struct RecoverableModelProvider {
    calls: Arc<AtomicUsize>,
    first_started: Arc<Notify>,
    release_first: Arc<Notify>,
    request_body: Mutex<Option<Vec<u8>>>,
}

struct ImmediateCountingProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for ImmediateCountingProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(provider_response("完成后恢复"))
    }
}

#[async_trait]
impl ProviderTransport for RecoverableModelProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => {
                *self.request_body.lock().unwrap() = Some(wire.body().to_vec());
                self.first_started.notify_one();
                self.release_first.notified().await;
                Ok(provider_response("过期调用不得落盘"))
            }
            1 => {
                assert_eq!(
                    self.request_body.lock().unwrap().as_deref(),
                    Some(wire.body()),
                    "retry must send the exact persisted request bytes"
                );
                Ok(provider_response("模型调用已恢复"))
            }
            call => panic!("unexpected provider call {call}"),
        }
    }
}

#[tokio::test]
async fn expired_started_model_call_retries_the_same_logical_effect() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, false, None, None).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"recover model"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-model-recovery-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let first_started = Arc::new(Notify::new());
    let release_first = Arc::new(Notify::new());
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(RecoverableModelProvider {
            calls: calls.clone(),
            first_started: first_started.clone(),
            release_first: release_first.clone(),
            request_body: Mutex::new(None),
        }),
    ));
    let now = now_ms();
    let first_scheduler = Scheduler::new(store.clone(), "model-recovery-old-worker")
        .with_llm_executor(executor.clone());
    let first_run = tokio::spawn(async move { first_scheduler.run_until_idle(now, 128).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), first_started.notified())
        .await
        .unwrap();
    assert!(
        SchedulerStore::recover_expired_leases(store.as_ref(), now + 31_000)
            .await
            .unwrap()
            >= 1
    );
    release_first.notify_waiters();
    assert!(
        first_run.await.unwrap().is_err(),
        "the stale fence must lose"
    );
    Scheduler::new(store.clone(), "model-recovery-new-worker")
        .with_llm_executor(executor)
        .run_until_idle(now + 31_001, 128)
        .await
        .unwrap();

    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. } if value == &json!("模型调用已恢复")
    ));
}

#[tokio::test]
async fn completed_model_call_is_finalized_after_lease_recovery_without_provider_replay() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, false, None, None).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"recover completed"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-completed-model-recovery-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let pause = Arc::new(CompletedModelPause::new());
    let executor = Arc::new(
        LocalLlmExecutor::with_provider(
            store.clone(),
            Arc::new(ImmediateCountingProvider {
                calls: calls.clone(),
            }),
        )
        .with_completed_model_pause(pause.clone()),
    );
    let now = now_ms();
    let old = Scheduler::new(store.clone(), "completed-model-old-worker")
        .with_llm_executor(executor.clone());
    let old_run = tokio::spawn(async move { old.run_until_idle(now, 128).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), pause.started.notified())
        .await
        .unwrap();
    assert!(
        SchedulerStore::recover_expired_leases(store.as_ref(), now + 31_000)
            .await
            .unwrap()
            >= 1
    );
    pause.release.notify_waiters();
    assert!(old_run.await.unwrap().is_err());
    Scheduler::new(store.clone(), "completed-model-new-worker")
        .with_llm_executor(executor)
        .run_until_idle(now + 31_001, 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. } if value == &json!("完成后恢复")
    ));
}
