use serde_json::json;

use crate::{
    DomainError,
    llm::{
        ChannelCredential, ChannelModel, ChannelModelCatalog, ChannelTransportPolicy,
        ContentGenerationKind, LlmChannelRevision, LlmChannelRevisionSpec, ModelCapabilities,
        ModelCatalogPolicy, Operation, OperationKey, revision_content_hash,
    },
};

use super::{GraphApplyDependencies, GraphDraft, apply_graph_with_dependencies};

fn operation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}

#[test]
fn llm_apply_rejects_explicit_false_even_with_override() {
    let draft: GraphDraft = serde_json::from_value(json!({
        "graphId":"graph_llm",
        "nodes":[
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {
                "id":"llm",
                "kind":"llm",
                "model":{
                    "channelId":"channel_1",
                    "modelId":"model_1",
                    "operationKey":{"operation":"generate_content","kind":"open_ai_responses"}
                },
                "capabilityOverrides":[{
                    "feature":"streaming",
                    "assumption":"supported",
                    "reason":"manual check",
                    "acknowledgementRef":"ack_1",
                    "policyVersion":1
                }],
                "context":{"type":"inline","spec":{"mode":"chat","items":[]}},
                "streaming":{"enabled":true,"audience":"internal"}
            },
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges":[
            {"from":{"nodeId":"input","output":"default"},"to":{"nodeId":"llm","input":"default"}},
            {"from":{"nodeId":"llm","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "outputContract":[{"key":"reply","collection":"single","required":true}]
    }))
    .unwrap();
    let spec = LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://example.test/v1".into(),
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
                id: "model_1".into(),
                name: None,
                context_window: Some(8_192),
                max_output_tokens: Some(1_024),
                capabilities: ModelCapabilities {
                    streaming: Some(false),
                    ..Default::default()
                },
            }],
        }],
        capabilities: vec![],
    };
    let hash = revision_content_hash(&spec).unwrap();
    let dependencies = GraphApplyDependencies {
        channel_heads: [(
            "channel_1".into(),
            LlmChannelRevision {
                id: "channelrev_1".into(),
                channel_id: "channel_1".into(),
                revision_no: 1,
                spec,
                content_hash: hash,
                created_at: 0,
            },
        )]
        .into(),
        preset_heads: Default::default(),
        tool_descriptors: Default::default(),
    };
    let error = apply_graph_with_dependencies(draft, 1, 1, &dependencies).unwrap_err();
    let DomainError::GraphValidation(issues) = error else {
        panic!("expected graph validation")
    };
    assert!(
        issues
            .iter()
            .any(|issue| issue.code == "required_capability_unsupported")
    );
}
