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
    StreamEventHub,
    llm_executor::LocalLlmExecutor,
    provider::{
        ProviderHttpError, ProviderHttpResponse, ProviderHttpStreamResponse, ProviderTransport,
    },
};

use super::{create_streaming_llm_graph, now_ms};

struct FixtureStreamProvider {
    truncated: bool,
}

#[async_trait]
impl ProviderTransport for FixtureStreamProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        panic!("streaming execution must not use the terminal transport")
    }

    async fn send_stream(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpStreamResponse, ProviderHttpError> {
        let request: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
        assert_eq!(request["stream"], true);
        let mut frames = response_frames();
        if self.truncated {
            frames.pop();
        }
        Ok(ProviderHttpStreamResponse {
            status: 200,
            provider_request_id: Some("stream-request-1".into()),
            frames: Box::pin(async_stream::stream! {
                for frame in frames {
                    yield Ok::<_, ProviderHttpError>(frame);
                }
            }),
        })
    }
}

#[tokio::test]
async fn streaming_model_call_emits_live_events_and_persists_bounded_chunks() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_streaming_llm_graph(&store, true).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"stream"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "streaming-run".into(),
        })
        .await
        .unwrap();
    let events = StreamEventHub::new();
    let mut live = events.subscribe();
    let executor = Arc::new(
        LocalLlmExecutor::with_provider(
            store.clone(),
            Arc::new(FixtureStreamProvider { truncated: false }),
        )
        .with_stream_events(events),
    );
    Scheduler::new(store.clone(), "stream-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();

    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. } if value == &json!("hello")
    ));
    let mut live_types = Vec::new();
    while let Ok(event) = live.try_recv() {
        live_types.push(event.event_type());
    }
    assert!(live_types.contains(&"llm.stream.started"));
    assert!(live_types.contains(&"llm.stream.text_delta"));
    assert!(live_types.contains(&"llm.stream.completed"));
    let durable = store.list_run_events(&run.id, 0, 500).await.unwrap();
    let chunks: Vec<_> = durable
        .iter()
        .filter(|event| event.event_type == "llm.stream.chunk")
        .collect();
    assert_eq!(
        chunks.len(),
        1,
        "small deltas must be coalesced into one chunk"
    );
    assert_eq!(chunks[0].payload["events"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn truncated_stream_never_becomes_a_completed_model_call() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_streaming_llm_graph(&store, false).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"truncate"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "truncated-stream-run".into(),
        })
        .await
        .unwrap();
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(FixtureStreamProvider { truncated: true }),
    ));
    Scheduler::new(store.clone(), "truncated-stream-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Failed
    );
    assert!(
        store
            .get_run_outputs(&run.id)
            .await
            .unwrap()
            .values()
            .all(|output| output.values.is_empty())
    );
    assert!(
        store
            .list_run_events(&run.id, 0, 500)
            .await
            .unwrap()
            .iter()
            .all(|event| event.event_type != "llm.stream.chunk")
    );
}

#[tokio::test]
async fn completed_stream_is_ephemeral_when_chunk_persistence_is_disabled() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_streaming_llm_graph(&store, false).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"ephemeral"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "ephemeral-stream-run".into(),
        })
        .await
        .unwrap();
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(FixtureStreamProvider { truncated: false }),
    ));
    Scheduler::new(store.clone(), "ephemeral-stream-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert!(
        store
            .list_run_events(&run.id, 0, 500)
            .await
            .unwrap()
            .iter()
            .all(|event| event.event_type != "llm.stream.chunk")
    );
}

fn response_frames() -> Vec<Vec<u8>> {
    [
        json!({
            "type":"response.created","sequence_number":0,
            "response":{"id":"response-1","created_at":1,"object":"response","output":[],"status":"in_progress"}
        }),
        json!({
            "type":"response.output_item.added","sequence_number":1,"output_index":0,
            "item":{"type":"message","id":"provider-message-1","role":"assistant","status":"in_progress","content":[]}
        }),
        json!({
            "type":"response.output_text.delta","sequence_number":2,"content_index":0,
            "delta":"hello","item_id":"provider-message-1","output_index":0
        }),
        json!({
            "type":"response.completed","sequence_number":3,
            "response":{
                "id":"response-1","created_at":1,"object":"response","status":"completed",
                "output":[{
                    "type":"message","id":"provider-message-1","role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":"hello","annotations":[]}]
                }],
                "usage":{"input_tokens":4,"output_tokens":1,"total_tokens":5,"output_tokens_details":{"reasoning_tokens":0}}
            }
        }),
    ]
    .into_iter()
    .map(|value| serde_json::to_vec(&value).unwrap())
    .collect()
}
