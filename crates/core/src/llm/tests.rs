use gproxy_protocol::{ContentGenerationKind, Operation, OperationKey};

use super::*;

fn generation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}

fn spec() -> LlmChannelRevisionSpec {
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://api.example.test/v1/".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: true,
        },
        credential: ChannelCredential::None,
        operation_keys: vec![generation()],
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: generation(),
            policy: ModelCatalogPolicy::Open,
            models: vec![],
        }],
        capabilities: vec![],
    }
}

#[test]
fn channel_normalization_is_closed_and_deterministic() {
    let normalized = normalize_channel_revision(spec()).unwrap();
    assert_eq!(normalized.base_url, "https://api.example.test/v1");
    let mut insecure = spec();
    insecure.base_url = "http://example.test/v1?api_key=x".into();
    assert!(normalize_channel_revision(insecure).is_err());
}

#[test]
fn unknown_capability_requires_override_but_false_cannot_be_overridden() {
    let revision = normalize_channel_revision(spec()).unwrap();
    let model_ref = LlmNodeModelRef {
        channel_id: "channel_1".into(),
        model_id: "custom-model".into(),
        model_name: None,
        operation_key: generation(),
    };
    let required = ModelCapabilityRequirements {
        streaming: true,
        ..Default::default()
    };
    assert!(validate_generation_model(&revision, &model_ref, &required, &[]).is_err());
    let capability_override = ModelCapabilityOverride {
        feature: ModelCapabilityName::Streaming,
        assumption: ModelCapabilityAssumption::Supported,
        reason: "endpoint documentation".into(),
        acknowledgement_ref: "ack_1".into(),
        policy_version: MODEL_CAPABILITY_POLICY_VERSION,
    };
    validate_generation_model(&revision, &model_ref, &required, &[capability_override]).unwrap();
}
