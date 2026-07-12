use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{Scheduler, SchedulerStore},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::{CountPause, LocalLlmExecutor},
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_llm_graph, now_ms, provider_response};

struct CountingProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for CountingProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(provider_response("count recovered"))
    }
}

#[derive(Clone, Copy)]
enum PausePoint {
    Prepared,
    Completed,
}

#[tokio::test]
async fn prepared_count_retries_the_same_logical_call_after_lease_recovery() {
    run_recovery(PausePoint::Prepared).await;
}

#[tokio::test]
async fn completed_count_is_reused_after_lease_recovery_before_model_prepare() {
    run_recovery(PausePoint::Completed).await;
}

async fn run_recovery(point: PausePoint) {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, false, None, None).await;
    let key = match point {
        PausePoint::Prepared => "prepared",
        PausePoint::Completed => "completed",
    };
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":format!("recover {key} count")}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: format!("server-{key}-count-recovery-run"),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let pause = Arc::new(CountPause::new());
    let executor = LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(CountingProvider {
            calls: calls.clone(),
        }),
    );
    let executor = Arc::new(match point {
        PausePoint::Prepared => executor.with_count_prepared_pause(pause.clone()),
        PausePoint::Completed => executor.with_count_completed_pause(pause.clone()),
    });
    let now = now_ms();
    let old = Scheduler::new(store.clone(), format!("{key}-count-old-worker"))
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
    Scheduler::new(store.clone(), format!("{key}-count-new-worker"))
        .with_llm_executor(executor)
        .run_until_idle(now + 31_001, 128)
        .await
        .unwrap();

    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    for event_type in ["llm.count.prepared", "llm.count.completed"] {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == event_type)
                .count(),
            1
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.count.retry_prepared")
            .count(),
        usize::from(matches!(point, PausePoint::Prepared))
    );
}
