use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    graph::{
        EffectClassification, HostedToolBinding, LlmMemoryBinding, LlmNodeStreaming,
        StreamingAudience, ToolEffectSpec, ToolGrant,
    },
    llm::{
        ChannelCapability, ChannelCredential, ChannelModel, ChannelModelCatalog,
        ChannelTransportPolicy, LlmChannelRevision, LlmChannelRevisionSpec, ModelCapabilities,
        ModelCatalogPolicy, Operation, OperationKey, Provider, SecretRef,
        adapter::WireGenerationRequest,
        context::{
            ContextAssemblyMode, ContextAssemblySpec, ContextBudgetPolicy, ContextBudgetStrategy,
            ContextItem, ContextPosition, ContextRole, ContextSource, OverflowPolicy,
            TokenBudgetHint,
        },
    },
    runtime::{RunContextCommand, RunOutputValueView, RunStatus, StartRunCommand},
    scheduler::Scheduler,
};

mod count_recovery;
mod counting_provider;
mod counting_provider_recovery;
mod graph_fixture;
mod hosted_tools;
mod memory_search_tools;
mod memory_tools;
mod model_recovery;
mod opaque_storage;
mod output_repair;
mod provider_fixture;
mod secret_wait;
mod streaming;
mod tool_recovery;
mod tool_registry;
use graph_fixture::graph_draft;
use provider_fixture::{now_ms, operation, provider_response};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

struct FakeProvider {
    text: &'static str,
}

struct CountAwareProvider {
    store: Arc<SqliteStore>,
    run_id: String,
}

#[async_trait]
impl ProviderTransport for FakeProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&zhuangsheng_core::application::secret::SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        Ok(provider_response(self.text))
    }
}

#[async_trait]
impl ProviderTransport for CountAwareProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&zhuangsheng_core::application::secret::SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let events = self
            .store
            .list_run_events(&self.run_id, 0, 200)
            .await
            .unwrap();
        assert!(events.iter().any(|event| {
            event.event_type == "llm.count.completed"
                && event.payload["resultSource"] == json!("estimate")
        }));
        Ok(provider_response("你好，旅行者。"))
    }
}

#[tokio::test]
async fn scheduler_executes_first_llm_call_through_durable_effect() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, false, None, None).await;
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
        Arc::new(CountAwareProvider {
            store: store.clone(),
            run_id: run.id.clone(),
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
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.count.completed")
            .count(),
        1
    );
}

#[tokio::test]
async fn scheduler_finalizes_strict_json_output() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, true, None, None).await;
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
    let view = store.get_run(&run.id).await.unwrap();
    let events = store.list_run_events(&run.id, 0, 200).await.unwrap();
    assert_eq!(view.status, RunStatus::Completed, "events: {events:#?}");
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!({"reply":"ok"})
    ));
}

async fn create_llm_graph(
    store: &SqliteStore,
    json_output: bool,
    credential: Option<SecretRef>,
    tool: Option<ToolGrant>,
) -> String {
    create_llm_graph_inner(
        store,
        LlmGraphFixture {
            json_output,
            credential,
            tool,
            ..Default::default()
        },
    )
    .await
}

async fn create_memory_llm_graph(store: &SqliteStore, memory: LlmMemoryBinding) -> String {
    create_llm_graph_inner(
        store,
        LlmGraphFixture {
            memory: Some(memory),
            ..Default::default()
        },
    )
    .await
}

async fn create_counting_llm_graph(store: &SqliteStore) -> String {
    create_llm_graph_inner(
        store,
        LlmGraphFixture {
            provider_count: true,
            ..Default::default()
        },
    )
    .await
}

async fn create_streaming_llm_graph(store: &SqliteStore, persist_chunks: bool) -> String {
    create_llm_graph_inner(
        store,
        LlmGraphFixture {
            streaming: Some(LlmNodeStreaming {
                enabled: true,
                audience: StreamingAudience::Both,
                persist_chunks,
            }),
            ..Default::default()
        },
    )
    .await
}

pub(super) async fn create_hosted_llm_graph(store: &SqliteStore) -> String {
    create_llm_graph_inner(
        store,
        LlmGraphFixture {
            hosted: Some(HostedToolBinding {
                binding_id: "search".into(),
                operation_key: operation(),
                hosted_kind: "web_search".into(),
                model_facing_config: [("search_context_size".into(), json!("low"))].into(),
                resource_scopes: vec!["internet:public".into()],
                effect: ToolEffectSpec {
                    classification: EffectClassification::Idempotent,
                    operation_key: "hosted.web_search".into(),
                    requires_approval: false,
                },
                max_uses_per_model_call: 1,
            }),
            ..Default::default()
        },
    )
    .await
}

#[derive(Default)]
struct LlmGraphFixture {
    json_output: bool,
    credential: Option<SecretRef>,
    tool: Option<ToolGrant>,
    hosted: Option<HostedToolBinding>,
    streaming: Option<LlmNodeStreaming>,
    memory: Option<LlmMemoryBinding>,
    provider_count: bool,
}

async fn create_llm_graph_inner(store: &SqliteStore, fixture: LlmGraphFixture) -> String {
    let LlmGraphFixture {
        json_output,
        credential,
        tool,
        hosted,
        streaming,
        memory,
        provider_count,
    } = fixture;
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
            spec: channel_spec(
                credential,
                tool.is_some()
                    || hosted.is_some()
                    || memory
                        .as_ref()
                        .is_some_and(|memory| !memory.tools.is_empty()),
                hosted.is_some(),
                streaming.is_some(),
                provider_count,
            ),
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
            spec: context_spec(provider_count),
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
            document: graph_draft(
                &graph.graph.id,
                &channel.id,
                &preset.id,
                LlmGraphFixture {
                    json_output,
                    credential: None,
                    tool,
                    hosted,
                    streaming,
                    memory,
                    provider_count,
                },
            ),
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

fn context_spec(with_optional: bool) -> ContextAssemblySpec {
    let mut items = vec![ContextItem {
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
    }];
    if with_optional {
        items.push(ContextItem {
            id: "optional-lore".into(),
            name: None,
            enabled: true,
            requested_role: ContextRole::Context,
            source: ContextSource::Literal {
                text: "lore ".repeat(800),
            },
            position: ContextPosition::Start,
            order: 0,
            priority: 1,
            insertion_depth: 0,
            budget: TokenBudgetHint {
                max_tokens: None,
                required: false,
            },
            overflow: Some(OverflowPolicy::Drop),
        });
    }
    ContextAssemblySpec {
        id: None,
        name: Some("Role Play".into()),
        mode: ContextAssemblyMode::Chat,
        items,
        budget: Some(ContextBudgetPolicy {
            max_input_tokens: None,
            strategy: Some(ContextBudgetStrategy::Strict),
        }),
        post_process: Vec::new(),
        text_transforms: Vec::new(),
        text_transform_macros: Default::default(),
        preview: None,
    }
}

fn channel_spec(
    credential: Option<SecretRef>,
    tool_calling: bool,
    hosted: bool,
    streaming: bool,
    provider_count: bool,
) -> LlmChannelRevisionSpec {
    let authenticated = credential.is_some();
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://fake.example.test/v1".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: !authenticated,
        },
        credential: credential.map_or(ChannelCredential::None, |api_key_ref| {
            ChannelCredential::Secret { api_key_ref }
        }),
        operation_keys: if provider_count {
            vec![
                operation(),
                OperationKey::provider(Operation::CountTokens, Provider::OpenAi),
            ]
        } else {
            vec![operation()]
        },
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: operation(),
            policy: ModelCatalogPolicy::Allowlist,
            models: vec![ChannelModel {
                id: "roleplay-model".into(),
                name: None,
                context_window: Some(16_384),
                max_output_tokens: Some(2_048),
                capabilities: ModelCapabilities {
                    streaming: streaming.then_some(true),
                    tool_calling: tool_calling.then_some(true),
                    structured_output: Some(true),
                    vision_input: None,
                },
            }],
        }],
        capabilities: hosted
            .then(|| ChannelCapability::HostedTool {
                operation_key: operation(),
                hosted_kind: "web_search".into(),
            })
            .into_iter()
            .collect(),
    }
}
