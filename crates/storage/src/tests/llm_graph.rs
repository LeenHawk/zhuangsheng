use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    graph::{DraftNodeKind, GraphDraft, LlmFinalText, LlmOutputSpec},
    llm::{
        ChannelCredential, ChannelModel, ChannelModelCatalog, ChannelTransportPolicy,
        ContentGenerationKind, LlmChannelRevisionSpec, ModelCapabilities, ModelCatalogPolicy,
        Operation, OperationKey, Provider,
        context::{ContextAssemblyMode, ContextAssemblySpec},
    },
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::Scheduler,
};

use crate::{graph::helpers::load_object_json, tests::store};

pub(super) fn operation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}

pub(super) fn count_operation() -> OperationKey {
    OperationKey::provider(Operation::CountTokens, Provider::OpenAi)
}

#[tokio::test]
async fn graph_apply_resolves_llm_channel_and_preset_heads() {
    let store = store().await;
    let channel = store
        .create_channel(CreateChannelCommand {
            name: "LLM".into(),
            idempotency_key: "llm-channel".into(),
        })
        .await
        .unwrap();
    let channel_v1 = store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id.clone(),
            expected_head_revision_id: None,
            spec: channel_spec(),
            idempotency_key: "llm-channel-revision".into(),
        })
        .await
        .unwrap();
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "RP".into(),
            idempotency_key: "llm-preset".into(),
        })
        .await
        .unwrap();
    let preset_v1 = store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: ContextAssemblySpec {
                id: None,
                name: None,
                mode: ContextAssemblyMode::Chat,
                items: vec![],
                budget: None,
                post_process: vec![],
                preview: None,
            },
            idempotency_key: "llm-preset-version".into(),
        })
        .await
        .unwrap();
    let graph = store
        .create_graph(CreateGraphCommand {
            name: "LLM Graph".into(),
            idempotency_key: "llm-graph".into(),
        })
        .await
        .unwrap();
    let draft = store.get_graph_draft(&graph.graph.id).await.unwrap();
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.graph.id.clone(),
            expected_revision_token: draft.revision_token,
            document: llm_draft(&graph.graph.id, &channel.id, &preset.id),
            idempotency_key: "llm-graph-draft".into(),
        })
        .await
        .unwrap();
    let revision = store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: "llm-graph-apply".into(),
        })
        .await
        .unwrap();
    let llm = revision
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "generate")
        .unwrap();
    let DraftNodeKind::Llm { config } = &llm.kind else {
        panic!("expected LLM node")
    };
    assert_eq!(llm.outputs[0].name, "default");
    assert!(matches!(
        config.output,
        Some(LlmOutputSpec::Text {
            final_text: Some(LlmFinalText::LastAssistantTurn),
            ..
        })
    ));
    assert_eq!(config.limits.as_ref().unwrap().max_model_calls, Some(8));

    let mut second_channel_spec = channel_spec();
    second_channel_spec.base_url = "https://llm-v2.example.test/v1".into();
    let channel_v2 = store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id,
            expected_head_revision_id: Some(channel_v1.id),
            spec: second_channel_spec,
            idempotency_key: "llm-channel-revision-v2".into(),
        })
        .await
        .unwrap();
    let mut second_preset_spec = preset_v1.spec.clone();
    second_preset_spec.name = Some("RP v2".into());
    let preset_v2 = store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id,
            expected_head_version_id: Some(preset_v1.id),
            spec: second_preset_spec,
            idempotency_key: "llm-preset-version-v2".into(),
        })
        .await
        .unwrap();
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "llm-run".into(),
        })
        .await
        .unwrap();
    Scheduler::new(std::sync::Arc::new(store.clone()), "llm-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    let row = store
        .db
        .query_one_raw(crate::graph::helpers::sql(
            "SELECT execution_snapshot_object_id, preset_version_id FROM node_instances WHERE run_id = ? AND node_id = 'generate'",
            vec![run.id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let object_id: String = row.try_get("", "execution_snapshot_object_id").unwrap();
    let pinned_preset: String = row.try_get("", "preset_version_id").unwrap();
    let snapshot: zhuangsheng_core::graph::LlmNodeExecutionSnapshot =
        load_object_json(&store.db, &object_id).await.unwrap();
    assert_eq!(snapshot.channel.id, channel_v2.id);
    assert_eq!(pinned_preset, preset_v2.id);
}

pub(super) fn channel_spec() -> LlmChannelRevisionSpec {
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://llm.example.test/v1".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: true,
        },
        credential: ChannelCredential::None,
        operation_keys: vec![operation(), count_operation()],
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: operation(),
            policy: ModelCatalogPolicy::Allowlist,
            models: vec![ChannelModel {
                id: "roleplay-model".into(),
                name: None,
                context_window: Some(16_384),
                max_output_tokens: Some(2_048),
                capabilities: ModelCapabilities::default(),
            }],
        }],
        capabilities: vec![],
    }
}

pub(super) fn llm_draft(graph_id: &str, channel_id: &str, preset_id: &str) -> GraphDraft {
    serde_json::from_value(json!({
        "graphId":graph_id,
        "nodes":[
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {
                "id":"generate",
                "kind":"llm",
                "model":{
                    "channelId":channel_id,
                    "modelId":"roleplay-model",
                    "operationKey":{
                        "operation":"generate_content",
                        "kind":"open_ai_responses"
                    }
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
    .unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
