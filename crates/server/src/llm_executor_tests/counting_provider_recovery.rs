use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Notify;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, Operation, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{Scheduler, SchedulerStore},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_counting_llm_graph, now_ms, provider_response};

struct RecoverableCountProvider {
    count_calls: AtomicUsize,
    first_started: Notify,
    release_first: Notify,
    request_body: Mutex<Option<Vec<u8>>>,
}

#[async_trait]
impl ProviderTransport for RecoverableCountProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        if wire.operation.operation_key.operation != Operation::CountTokens {
            return Ok(provider_response("provider count recovered"));
        }
        match self.count_calls.fetch_add(1, Ordering::SeqCst) {
            0 => {
                *self.request_body.lock().unwrap() = Some(wire.body().to_vec());
                self.first_started.notify_one();
                self.release_first.notified().await;
            }
            1 => assert_eq!(
                self.request_body.lock().unwrap().as_deref(),
                Some(wire.body()),
                "count retry must reproduce the pinned provider request",
            ),
            call => panic!("unexpected provider count call {call}"),
        }
        Ok(ProviderHttpResponse {
            status: 200,
            provider_request_id: Some("provider-count-recovery".into()),
            body: br#"{"input_tokens":41}"#.to_vec(),
        })
    }
}

#[tokio::test]
async fn expired_started_provider_count_retries_one_logical_call() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_counting_llm_graph(&store).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"recover provider count"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "provider-count-recovery-run".into(),
        })
        .await
        .unwrap();
    let provider = Arc::new(RecoverableCountProvider {
        count_calls: AtomicUsize::new(0),
        first_started: Notify::new(),
        release_first: Notify::new(),
        request_body: Mutex::new(None),
    });
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        provider.clone(),
    ));
    let now = now_ms();
    let old = Scheduler::new(store.clone(), "provider-count-old-worker")
        .with_llm_executor(executor.clone());
    let old_run = tokio::spawn(async move { old.run_until_idle(now, 128).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        provider.first_started.notified(),
    )
    .await
    .unwrap();
    assert!(
        SchedulerStore::recover_expired_leases(store.as_ref(), now + 31_000)
            .await
            .unwrap()
            >= 1
    );
    provider.release_first.notify_waiters();
    assert!(old_run.await.unwrap().is_err());
    Scheduler::new(store.clone(), "provider-count-new-worker")
        .with_llm_executor(executor)
        .run_until_idle(now + 31_001, 128)
        .await
        .unwrap();

    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(provider.count_calls.load(Ordering::SeqCst), 2);
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.count.prepared")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.count.retry_prepared")
            .count(),
        1
    );
    let completed = events
        .iter()
        .find(|event| event.event_type == "llm.count.completed")
        .unwrap();
    assert_eq!(completed.payload["resultSource"], json!("provider"));
}
