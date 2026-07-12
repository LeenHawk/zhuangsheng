use std::collections::BTreeMap;

use gproxy_protocol::{Operation, OperationGroup, OperationKey, Provider};

use super::{
    ChannelCredential, ChannelModel, ChannelModelCatalog, ChannelTransportPolicy,
    CompactOperationPlan, CompactPurpose, EmbeddingInputRef, EmbeddingOperationPlan,
    ImageOperationPlan, LlmChannelRevision, LlmChannelRevisionSpec, LlmNodeModelRef,
    ModelCapabilities, ModelCatalogPolicy, normalize_channel_revision, prepare_embedding_operation,
    resolve_service_operation, validate_compact_plan, validate_embedding_plan, validate_image_plan,
};

#[test]
fn channel_accepts_only_the_phase_one_service_operation_shapes() {
    for key in [image_key(), embedding_key(), compact_key()] {
        normalize_channel_revision(spec(key)).unwrap();
    }
    let mut unsupported = spec(OperationKey::provider(
        Operation::CreateImage,
        Provider::Gemini,
    ));
    unsupported.model_catalogs[0].operation_key = unsupported.operation_keys[0];
    assert_eq!(
        normalize_channel_revision(unsupported).unwrap_err().code,
        "unsupported_channel_operation"
    );
}

#[test]
fn service_model_resolution_returns_an_exact_revision_pin() {
    let revision = revision(embedding_key());
    let model = model_ref(embedding_key());
    let pin = resolve_service_operation(&revision, &model, OperationGroup::Embeddings).unwrap();
    assert_eq!(pin.channel_revision_id, revision.id);
    assert_eq!(pin.model_id, "service-model");
    assert_eq!(pin.operation_key, embedding_key());
    assert_eq!(
        resolve_service_operation(&revision, &model, OperationGroup::Images)
            .unwrap_err()
            .code,
        "service_operation_not_allowed"
    );
}

#[test]
fn service_plans_are_bounded_and_keep_secret_like_options_closed() {
    let mut image = ImageOperationPlan {
        model: model_ref(image_key()),
        prompt_ref: "object:prompt".into(),
        options: BTreeMap::new(),
        max_images: 2,
        max_total_bytes: 8 * 1024 * 1024,
    };
    validate_image_plan(&image).unwrap();
    image.options.insert(
        "api_token".into(),
        super::ir::MetadataValue::String("secret".into()),
    );
    assert_eq!(
        validate_image_plan(&image).unwrap_err().code,
        "invalid_image_plan"
    );

    let hash = format!("sha256:{}", "a".repeat(64));
    let embedding = EmbeddingOperationPlan {
        model: model_ref(embedding_key()),
        inputs: vec![EmbeddingInputRef {
            source_ref: "object:source".into(),
            content_hash: hash,
        }],
        dimensions: Some(1536),
    };
    validate_embedding_plan(&embedding).unwrap();
    let prepared = prepare_embedding_operation(&revision(embedding_key()), embedding).unwrap();
    assert_eq!(prepared.operation.operation_key, embedding_key());

    let compact = CompactOperationPlan {
        model: model_ref(compact_key()),
        input_refs: vec!["object:history".into()],
        target_tokens: 1024,
        purpose: CompactPurpose::History,
    };
    validate_compact_plan(&compact).unwrap();
}

fn image_key() -> OperationKey {
    OperationKey::provider(Operation::CreateImage, Provider::OpenAi)
}

fn embedding_key() -> OperationKey {
    OperationKey::provider(Operation::CreateEmbedding, Provider::Gemini)
}

fn compact_key() -> OperationKey {
    OperationKey::provider(Operation::CompactContent, Provider::OpenAi)
}

fn model_ref(key: OperationKey) -> LlmNodeModelRef {
    LlmNodeModelRef {
        channel_id: "channel".into(),
        model_id: "service-model".into(),
        model_name: None,
        operation_key: key,
    }
}

fn spec(key: OperationKey) -> LlmChannelRevisionSpec {
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://example.test/v1".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: true,
        },
        credential: ChannelCredential::None,
        operation_keys: vec![key],
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: key,
            policy: ModelCatalogPolicy::Allowlist,
            models: vec![ChannelModel {
                id: "service-model".into(),
                name: None,
                context_window: None,
                max_output_tokens: None,
                capabilities: ModelCapabilities::default(),
            }],
        }],
        capabilities: vec![],
    }
}

fn revision(key: OperationKey) -> LlmChannelRevision {
    let spec = normalize_channel_revision(spec(key)).unwrap();
    LlmChannelRevision {
        id: "channel-revision".into(),
        channel_id: "channel".into(),
        revision_no: 1,
        content_hash: super::revision_content_hash(&spec).unwrap(),
        spec,
        created_at: 0,
    }
}
