use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunOutputValueView, RunStatus, StartRunCommand},
    scheduler::Scheduler,
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_hosted_llm_graph, now_ms};

struct HostedSearchProvider;

#[async_trait]
impl ProviderTransport for HostedSearchProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let request: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
        assert_eq!(request["tools"][0]["type"], "web_search");
        assert_eq!(request["tools"][0]["search_context_size"], "low");
        Ok(ProviderHttpResponse {
            status: 200,
            provider_request_id: Some("hosted-request".into()),
            body: response_body(),
        })
    }
}

#[tokio::test]
async fn scheduler_executes_allowlisted_hosted_web_search() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_hosted_llm_graph(&store).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"search lore"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "hosted-search-run".into(),
        })
        .await
        .unwrap();
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(HostedSearchProvider),
    ));
    Scheduler::new(store.clone(), "hosted-search-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    let view = store.get_run(&run.id).await.unwrap();
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    assert_eq!(view.status, RunStatus::Completed, "events: {events:#?}");
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!("我找到了新的世界设定。")
    ));
}

fn response_body() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id":"response-hosted-1",
        "created_at":1,
        "object":"response",
        "output":[
            {
                "type":"web_search_call",
                "id":"search-1",
                "status":"completed",
                "action":{"type":"search","query":"latest lore"}
            },
            {
                "type":"message",
                "id":"message-1",
                "role":"assistant",
                "status":"completed",
                "content":[{"type":"output_text","text":"我找到了新的世界设定。","annotations":[]}]
            }
        ],
        "status":"completed",
        "usage":{
            "input_tokens":12,
            "output_tokens":7,
            "total_tokens":19,
            "output_tokens_details":{"reasoning_tokens":0}
        }
    }))
    .unwrap()
}
