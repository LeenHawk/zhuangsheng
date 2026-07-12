use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::{
        InitializeSecretStoreCommand, LockSecretStoreCommand, PutSecretCommand, SecretKind,
        SecretValue, UnlockSecretStoreCommand,
    },
    llm::{LlmChannelRevision, SecretRef, SecretScheme, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunStatus, StartRunCommand, WaitKind},
    scheduler::Scheduler,
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_llm_graph, now_ms, provider_response};

struct CredentialProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for CredentialProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        credential
            .expect("credential must be resolved before provider send")
            .with_bytes(|bytes| assert_eq!(bytes, b"sk-secret-wait-e2e"));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(provider_response("解锁后继续。"))
    }
}

#[tokio::test]
async fn locked_secret_waits_before_model_effect_and_unlock_resumes_automatically() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let session = store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: SecretValue::from_utf8("secret-wait-password".into()),
            idempotency_key: "secret-wait-init".into(),
        })
        .await
        .unwrap();
    store
        .put_secret_record(PutSecretCommand {
            secret_id: "secret-wait-api-key".into(),
            name: None,
            kind: SecretKind::ApiKey,
            value: SecretValue::from_utf8("sk-secret-wait-e2e".into()),
            session_id: session.session_id.clone(),
            idempotency_key: "secret-wait-put".into(),
        })
        .await
        .unwrap();
    store
        .lock_secret_store(LockSecretStoreCommand {
            expected_session_id: Some(session.session_id),
            idempotency_key: "secret-wait-lock".into(),
        })
        .await
        .unwrap();
    let revision_id = create_llm_graph(
        &store,
        false,
        Some(SecretRef {
            scheme: SecretScheme::Secret,
            id: "secret-wait-api-key".into(),
        }),
        None,
    )
    .await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"wait"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "secret-wait-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(CredentialProvider {
            calls: calls.clone(),
        }),
    ));
    let scheduler = Scheduler::new(store.clone(), "secret-wait-worker").with_llm_executor(executor);
    scheduler.run_until_idle(now_ms(), 128).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Waiting
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let waits = store.list_open_waits(&run.id).await.unwrap();
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].kind, WaitKind::SecretStoreUnlocked);
    let request = serde_json::to_string(&waits[0].request).unwrap();
    assert!(!request.contains("secret-wait-password"));
    assert!(!request.contains("sk-secret-wait-e2e"));

    store
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: SecretValue::from_utf8("secret-wait-password".into()),
            idempotency_key: "secret-wait-unlock".into(),
        })
        .await
        .unwrap();
    scheduler.run_until_idle(now_ms(), 128).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(store.list_open_waits(&run.id).await.unwrap().is_empty());
    let events =
        serde_json::to_string(&store.list_run_events(&run.id, 0, 500).await.unwrap()).unwrap();
    assert!(!events.contains("secret-wait-password"));
    assert!(!events.contains("sk-secret-wait-e2e"));
}
