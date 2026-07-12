use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, Operation, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::Scheduler,
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_counting_llm_graph, now_ms, provider_response};

struct ProviderCounter {
    operations: Mutex<Vec<Operation>>,
    malformed_count: bool,
}

struct OverageProvider {
    operations: Mutex<Vec<Operation>>,
    count_bodies: Mutex<Vec<Vec<u8>>>,
}

#[async_trait]
impl ProviderTransport for ProviderCounter {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let operation = wire.operation.operation_key.operation;
        self.operations.lock().unwrap().push(operation);
        if operation == Operation::CountTokens {
            assert_eq!(wire.relative_path, "/v1/responses/input_tokens");
            let body: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
            assert!(body.get("input").is_some());
            assert!(body.get("max_output_tokens").is_none());
            return Ok(ProviderHttpResponse {
                status: 200,
                provider_request_id: Some("count-request".into()),
                body: if self.malformed_count {
                    br#"{"unexpected":1}"#.to_vec()
                } else {
                    br#"{"input_tokens":37,"object":"response.input_tokens"}"#.to_vec()
                },
            });
        }
        Ok(provider_response("provider counted"))
    }
}

#[async_trait]
impl ProviderTransport for OverageProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let operation = wire.operation.operation_key.operation;
        self.operations.lock().unwrap().push(operation);
        if operation == Operation::CountTokens {
            let count_no = {
                let mut bodies = self.count_bodies.lock().unwrap();
                bodies.push(wire.body().to_vec());
                bodies.len()
            };
            return Ok(ProviderHttpResponse {
                status: 200,
                provider_request_id: Some(format!("count-{count_no}")),
                body: serde_json::to_vec(&json!({
                    "input_tokens":if count_no == 1 { 25_000 } else { 100 }
                }))
                .unwrap(),
            });
        }
        Ok(provider_response("trimmed before generation"))
    }
}

#[tokio::test]
async fn provider_count_completes_before_generation() {
    run_count(false, "provider").await;
}

#[tokio::test]
async fn malformed_provider_count_falls_back_to_honest_estimate() {
    run_count(true, "estimate").await;
}

#[tokio::test]
async fn provider_overage_reassembles_one_new_trim_candidate() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_counting_llm_graph(&store).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"trim optional lore"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "provider-count-overage".into(),
        })
        .await
        .unwrap();
    let provider = Arc::new(OverageProvider {
        operations: Mutex::new(Vec::new()),
        count_bodies: Mutex::new(Vec::new()),
    });
    Scheduler::new(store.clone(), "provider-overage-worker")
        .with_llm_executor(Arc::new(LocalLlmExecutor::with_provider(
            store.clone(),
            provider.clone(),
        )))
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        provider.operations.lock().unwrap().as_slice(),
        &[
            Operation::CountTokens,
            Operation::CountTokens,
            Operation::GenerateContent,
        ]
    );
    {
        let bodies = provider.count_bodies.lock().unwrap();
        assert_eq!(bodies.len(), 2);
        assert!(bodies[1].len() < bodies[0].len());
    }
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.count.completed")
            .count(),
        2
    );
}

async fn run_count(malformed_count: bool, expected_source: &str) {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_counting_llm_graph(&store).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"count me"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: format!("provider-count-{expected_source}"),
        })
        .await
        .unwrap();
    let provider = Arc::new(ProviderCounter {
        operations: Mutex::new(Vec::new()),
        malformed_count,
    });
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        provider.clone(),
    ));
    Scheduler::new(store.clone(), "provider-count-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();

    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        provider.operations.lock().unwrap().as_slice(),
        &[Operation::CountTokens, Operation::GenerateContent]
    );
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    let completed = events
        .iter()
        .find(|event| event.event_type == "llm.count.completed")
        .unwrap();
    assert_eq!(completed.payload["resultSource"], json!(expected_source));
}
