use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    graph::{DraftNodeKind, GraphDraft, LlmOutputSpec},
    llm::{
        ChannelCredential, ChannelModel, ChannelModelCatalog, ChannelTransportPolicy,
        ContentGenerationKind, LlmChannelRevision, LlmChannelRevisionSpec, ModelCapabilities,
        ModelCatalogPolicy, Operation, OperationKey,
        adapter::WireGenerationRequest,
        context::{
            ContextAssemblyMode, ContextAssemblySpec, ContextBudgetPolicy, ContextBudgetStrategy,
            ContextItem, ContextPosition, ContextRole, ContextSource, TokenBudgetHint,
        },
    },
    runtime::{RunContextCommand, RunOutputValueView, RunStatus, StartRunCommand},
    scheduler::Scheduler,
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

struct FakeProvider {
    text: &'static str,
}

#[async_trait]
impl ProviderTransport for FakeProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&zhuangsheng_core::application::secret::SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        Ok(ProviderHttpResponse {
            status: 200,
            provider_request_id: Some("request-test".into()),
            body: serde_json::to_vec(&json!({
                "id":"response-1",
                "created_at":1,
                "object":"response",
                "output":[{
                    "type":"message",
                    "id":"message-1",
                    "role":"assistant",
                    "status":"completed",
                    "content":[{"type":"output_text","text":self.text,"annotations":[]}]
                }],
                "status":"completed",
                "usage":{
                    "input_tokens":12,
                    "output_tokens":7,
                    "total_tokens":19,
                    "output_tokens_details":{"reasoning_tokens":0}
                }
            }))
            .unwrap(),
        })
    }
}

#[tokio::test]
async fn scheduler_executes_first_llm_call_through_durable_effect() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, false).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "llm-e2e-run".into(),
        })
        .await
        .unwrap();
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(FakeProvider {
            text: "你好，旅行者。",
        }),
    ));
    Scheduler::new(store.clone(), "llm-e2e-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    let view = store.get_run(&run.id).await.unwrap();
    assert_eq!(view.status, RunStatus::Completed);
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    let value = &outputs["reply"].values[0];
    assert!(matches!(
        value,
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!("你好，旅行者。")
    ));
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "node.completed")
    );
}

#[tokio::test]
async fn scheduler_finalizes_strict_json_output() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, true).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"json"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "llm-json-run".into(),
        })
        .await
        .unwrap();
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(FakeProvider {
            text: r#"{"reply":"ok"}"#,
        }),
    ));
    Scheduler::new(store.clone(), "llm-json-worker")
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
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!({"reply":"ok"})
    ));
}

async fn create_llm_graph(store: &SqliteStore, json_output: bool) -> String {
    let channel = store
        .create_channel(CreateChannelCommand {
            name: "Fake LLM".into(),
            idempotency_key: "llm-e2e-channel".into(),
        })
        .await
        .unwrap();
    store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id.clone(),
            expected_head_revision_id: None,
            spec: channel_spec(),
            idempotency_key: "llm-e2e-channel-revision".into(),
        })
        .await
        .unwrap();
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "RP".into(),
            idempotency_key: "llm-e2e-preset".into(),
        })
        .await
        .unwrap();
    store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: context_spec(),
            idempotency_key: "llm-e2e-preset-version".into(),
        })
        .await
        .unwrap();
    let graph = store
        .create_graph(CreateGraphCommand {
            name: "LLM E2E".into(),
            idempotency_key: "llm-e2e-graph".into(),
        })
        .await
        .unwrap();
    let current = store.get_graph_draft(&graph.graph.id).await.unwrap();
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.graph.id.clone(),
            expected_revision_token: current.revision_token,
            document: graph_draft(&graph.graph.id, &channel.id, &preset.id, json_output),
            idempotency_key: "llm-e2e-draft".into(),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: "llm-e2e-apply".into(),
        })
        .await
        .unwrap()
        .id
}

fn context_spec() -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: None,
        name: Some("Role Play".into()),
        mode: ContextAssemblyMode::Chat,
        items: vec![ContextItem {
            id: "user-input".into(),
            name: None,
            enabled: true,
            requested_role: ContextRole::User,
            source: ContextSource::Input {
                path: "/default/message".into(),
            },
            position: ContextPosition::UserInput,
            order: 0,
            priority: 100,
            insertion_depth: 0,
            budget: TokenBudgetHint {
                max_tokens: None,
                required: true,
            },
            overflow: None,
        }],
        budget: Some(ContextBudgetPolicy {
            max_input_tokens: None,
            strategy: Some(ContextBudgetStrategy::Strict),
        }),
        post_process: Vec::new(),
        preview: None,
    }
}

fn channel_spec() -> LlmChannelRevisionSpec {
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://fake.example.test/v1".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: true,
        },
        credential: ChannelCredential::None,
        operation_keys: vec![operation()],
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: operation(),
            policy: ModelCatalogPolicy::Allowlist,
            models: vec![ChannelModel {
                id: "roleplay-model".into(),
                name: None,
                context_window: Some(16_384),
                max_output_tokens: Some(2_048),
                capabilities: ModelCapabilities {
                    streaming: None,
                    tool_calling: None,
                    structured_output: Some(true),
                    vision_input: None,
                },
            }],
        }],
        capabilities: Vec::new(),
    }
}

fn graph_draft(graph_id: &str, channel_id: &str, preset_id: &str, json_output: bool) -> GraphDraft {
    let mut draft: GraphDraft = serde_json::from_value(json!({
        "graphId":graph_id,
        "nodes":[
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {
                "id":"generate",
                "kind":"llm",
                "model":{
                    "channelId":channel_id,
                    "modelId":"roleplay-model",
                    "operationKey":{"operation":"generate_content","kind":"open_ai_responses"}
                },
                "context":{"type":"preset","presetId":preset_id}
            },
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges":[
            {"from":{"nodeId":"input","output":"default"},"to":{"nodeId":"generate","input":"default"}},
            {"from":{"nodeId":"generate","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "outputContract":[{"key":"reply","collection":"single","required":true}]
    }))
    .unwrap();
    if json_output {
        let config = draft
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.kind {
                DraftNodeKind::Llm { config } => Some(config),
                _ => None,
            })
            .unwrap();
        config.output = Some(LlmOutputSpec::Json {
            schema: JsonSchemaSpec {
                schema_version: 1,
                dialect: DIALECT_2020_12.into(),
                validation_profile_version: 1,
                format_policy_version: 1,
                document: json!({
                    "type":"object",
                    "required":["reply"],
                    "additionalProperties":false,
                    "properties":{"reply":{"type":"string"}}
                }),
                limits: JsonSchemaLimits::default(),
            },
            strict: true,
        });
    }
    draft
}

fn operation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
