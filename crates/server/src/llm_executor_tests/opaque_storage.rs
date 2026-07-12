use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::{InitializeSecretStoreCommand, SecretValue},
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunOutputValueView, RunStatus, StartRunCommand},
    scheduler::Scheduler,
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_llm_graph, now_ms, provider_response};

const MARKER: &str = "opaque-secret-marker-never-plaintext";

struct OpaqueRepairProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for OpaqueRepairProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(opaque_response("not json")),
            1 => {
                let request: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
                let input = request["input"].as_array().unwrap();
                let restored = input
                    .iter()
                    .find(|item| item["type"] == "reasoning")
                    .expect("the provider reasoning item must survive the durable repair turn");
                assert_eq!(restored["encrypted_content"], MARKER);
                assert!(
                    request["input"]
                        .to_string()
                        .contains("llm_json_parse_failed")
                );
                Ok(provider_response(
                    r#"{"reply":"fixed with opaque continuation"}"#,
                ))
            }
            call => panic!("unexpected provider call {call}"),
        }
    }
}

#[tokio::test]
async fn output_repair_restores_encrypted_provider_reasoning() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let store = Arc::new(SqliteStore::connect(&url).await.unwrap());
    store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: SecretValue::from_utf8("opaque-storage-password".into()),
            idempotency_key: "opaque-storage-initialize".into(),
        })
        .await
        .unwrap();
    let revision_id = create_llm_graph(&store, true, None, None).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"preserve reasoning"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "opaque-storage-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(OpaqueRepairProvider {
            calls: calls.clone(),
        }),
    ));
    Scheduler::new(store.clone(), "opaque-storage-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();

    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed,
        "events: {events:#?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!({"reply":"fixed with opaque continuation"})
    ));
    drop(store);
    assert_files_do_not_contain(&path, MARKER.as_bytes());
}

fn assert_files_do_not_contain(path: &std::path::Path, needle: &[u8]) {
    for candidate in [
        path.to_owned(),
        std::path::PathBuf::from(format!("{}-wal", path.display())),
        std::path::PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            assert!(!bytes.windows(needle.len()).any(|window| window == needle));
        }
    }
}

fn opaque_response(text: &str) -> ProviderHttpResponse {
    ProviderHttpResponse {
        status: 200,
        provider_request_id: Some("opaque-request".into()),
        body: serde_json::to_vec(&json!({
            "id":"response-opaque-1",
            "created_at":1,
            "object":"response",
            "output":[
                {
                    "type":"reasoning",
                    "id":"reasoning-1",
                    "status":"completed",
                    "summary":[{"type":"summary_text","text":"private summary"}],
                    "encrypted_content":MARKER
                },
                {
                    "type":"message",
                    "id":"message-1",
                    "role":"assistant",
                    "status":"completed",
                    "content":[{"type":"output_text","text":text,"annotations":[]}]
                }
            ],
            "status":"completed",
            "usage":{
                "input_tokens":12,
                "output_tokens":7,
                "total_tokens":19,
                "output_tokens_details":{"reasoning_tokens":1}
            }
        }))
        .unwrap(),
    }
}
