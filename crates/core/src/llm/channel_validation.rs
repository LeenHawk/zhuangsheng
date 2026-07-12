use std::collections::HashSet;

use gproxy_protocol::{ContentGenerationKind, Operation, OperationKey, OperationKind, Provider};
use url::{Host, Url};

use crate::{canonical, compatibility::supports_operation_versions};

use super::{
    ChannelCredential, ChannelTransportPolicy, LlmChannelRevisionSpec, LlmConfigError,
    LlmConfigResult, LlmNodeModelRef, MODEL_CAPABILITY_POLICY_VERSION, ModelCapabilityOverride,
    ModelCapabilityRequirements, ModelCatalogPolicy,
};

pub fn normalize_channel_revision(
    mut spec: LlmChannelRevisionSpec,
) -> LlmConfigResult<LlmChannelRevisionSpec> {
    if !supports_operation_versions(
        spec.operation_taxonomy_version,
        spec.adapter_decoder_version,
    ) {
        return Err(version_error(&spec));
    }
    spec.base_url = normalize_base_url(&spec.base_url, &spec.transport_policy)?;
    match &spec.credential {
        ChannelCredential::Secret { api_key_ref } => api_key_ref.validate()?,
        ChannelCredential::None if !spec.transport_policy.allow_unauthenticated => {
            return Err(LlmConfigError::new(
                "unauthenticated_channel_denied",
                "credential type none requires an explicit channel policy",
            ));
        }
        ChannelCredential::None => {}
    }
    normalize_operations(&mut spec)?;
    normalize_catalogs(&mut spec)?;
    normalize_capabilities(&mut spec)?;
    Ok(spec)
}

pub fn validate_generation_model(
    revision: &LlmChannelRevisionSpec,
    model_ref: &LlmNodeModelRef,
    requirements: &ModelCapabilityRequirements,
    overrides: &[ModelCapabilityOverride],
) -> LlmConfigResult<()> {
    if model_ref.channel_id.is_empty() || model_ref.model_id.trim().is_empty() {
        return Err(LlmConfigError::new(
            "invalid_model_ref",
            "channel id and model id are required",
        ));
    }
    if !is_supported_generation_key(model_ref.operation_key) {
        return Err(LlmConfigError::new(
            "unsupported_generation_operation",
            "LLMNode requires one of the four supported generation shapes",
        ));
    }
    if !revision.operation_keys.contains(&model_ref.operation_key) {
        return Err(LlmConfigError::new(
            "channel_operation_not_declared",
            "channel revision does not declare the node operation",
        ));
    }
    let catalog = revision
        .model_catalogs
        .iter()
        .find(|catalog| catalog.operation_key == model_ref.operation_key)
        .ok_or_else(|| {
            LlmConfigError::new(
                "model_catalog_missing",
                "channel revision has no model catalog for the node operation",
            )
        })?;
    let model = catalog
        .models
        .iter()
        .find(|model| model.id == model_ref.model_id);
    if catalog.policy == ModelCatalogPolicy::Allowlist && model.is_none() {
        return Err(LlmConfigError::new(
            "model_not_allowed",
            "model is absent from the channel allowlist",
        ));
    }
    validate_overrides(overrides)?;
    for feature in requirements.required() {
        match model.and_then(|model| model.capabilities.get(feature)) {
            Some(true) => {}
            Some(false) => {
                return Err(LlmConfigError::new(
                    "required_capability_unsupported",
                    format!("model explicitly reports {feature:?} as unsupported"),
                ));
            }
            None if overrides.iter().any(|item| item.feature == feature) => {}
            None => {
                return Err(LlmConfigError::new(
                    "required_capability_unknown",
                    format!("model capability {feature:?} requires an explicit override"),
                ));
            }
        }
    }
    Ok(())
}

pub fn is_supported_generation_key(key: OperationKey) -> bool {
    matches!(
        key.operation,
        Operation::GenerateContent | Operation::StreamGenerateContent
    ) && matches!(
        key.kind,
        OperationKind::ContentGeneration(
            ContentGenerationKind::OpenAiResponses
                | ContentGenerationKind::OpenAiChatCompletions
                | ContentGenerationKind::ClaudeMessages
                | ContentGenerationKind::GeminiGenerateContent
        )
    )
}

pub fn is_supported_count_key(key: OperationKey) -> bool {
    key.operation == Operation::CountTokens
        && matches!(
            key.kind,
            OperationKind::Provider(Provider::OpenAi | Provider::Claude | Provider::Gemini)
        )
}

fn validate_overrides(overrides: &[ModelCapabilityOverride]) -> LlmConfigResult<()> {
    let mut seen = HashSet::new();
    for capability_override in overrides {
        if !seen.insert(capability_override.feature)
            || capability_override.policy_version != MODEL_CAPABILITY_POLICY_VERSION
            || capability_override.reason.trim().is_empty()
            || capability_override.acknowledgement_ref.trim().is_empty()
        {
            return Err(LlmConfigError::new(
                "invalid_capability_override",
                "capability overrides must be unique, acknowledged, and use the current policy",
            ));
        }
    }
    Ok(())
}

fn version_error(spec: &LlmChannelRevisionSpec) -> LlmConfigError {
    let code =
        if spec.operation_taxonomy_version != crate::compatibility::OPERATION_TAXONOMY_VERSION {
            "unsupported_operation_taxonomy"
        } else {
            "unsupported_adapter_decoder"
        };
    LlmConfigError::new(code, "channel revision uses an unsupported version pair")
}

fn normalize_base_url(raw: &str, policy: &ChannelTransportPolicy) -> LlmConfigResult<String> {
    let mut url = Url::parse(raw.trim())
        .map_err(|_| LlmConfigError::new("invalid_channel_base_url", "base URL is not absolute"))?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(LlmConfigError::new(
            "invalid_channel_base_url",
            "base URL cannot contain userinfo or a fragment",
        ));
    }
    if url.query_pairs().any(|(key, _)| sensitive_query_key(&key)) {
        return Err(LlmConfigError::new(
            "sensitive_channel_query",
            "base URL query contains a credential-like field",
        ));
    }
    let loopback = match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    match url.scheme() {
        "https" => {}
        "http" if loopback && policy.allow_loopback_http => {}
        _ => {
            return Err(LlmConfigError::new(
                "insecure_channel_base_url",
                "base URL must use HTTPS; only explicit loopback HTTP is allowed",
            ));
        }
    }
    if url.path() != "/" {
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn sensitive_query_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    ["key", "token", "secret", "signature", "credential"]
        .iter()
        .any(|needle| normalized.contains(needle))
}

fn operation_sort_key(key: OperationKey) -> LlmConfigResult<String> {
    canonical::to_string(&key)
        .map_err(|error| LlmConfigError::new("invalid_operation_key", error.to_string()))
}

fn normalize_operations(spec: &mut LlmChannelRevisionSpec) -> LlmConfigResult<()> {
    if spec.operation_keys.is_empty() || spec.operation_keys.len() > 64 {
        return Err(LlmConfigError::new(
            "invalid_channel_operations",
            "channel revision requires 1..=64 operations",
        ));
    }
    if spec.operation_keys.iter().any(|key| !key.is_consistent()) {
        return Err(LlmConfigError::new(
            "invalid_operation_key",
            "operation and wire kind are inconsistent",
        ));
    }
    if spec
        .operation_keys
        .iter()
        .any(|key| !is_supported_generation_key(*key) && !is_supported_count_key(*key))
    {
        return Err(LlmConfigError::new(
            "unsupported_channel_operation",
            "the current adapter registry does not support a declared operation shape",
        ));
    }
    let mut keyed = spec
        .operation_keys
        .drain(..)
        .map(|key| Ok((operation_sort_key(key)?, key)))
        .collect::<LlmConfigResult<Vec<_>>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    if keyed.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(LlmConfigError::new(
            "duplicate_channel_operation",
            "channel operation keys must be unique",
        ));
    }
    spec.operation_keys = keyed.into_iter().map(|(_, key)| key).collect();
    Ok(())
}

fn normalize_catalogs(spec: &mut LlmChannelRevisionSpec) -> LlmConfigResult<()> {
    if spec.model_catalogs.len() > 64 {
        return Err(LlmConfigError::new(
            "model_catalog_limit",
            "channel has too many model catalogs",
        ));
    }
    for catalog in &mut spec.model_catalogs {
        if !spec.operation_keys.contains(&catalog.operation_key) {
            return Err(LlmConfigError::new(
                "catalog_operation_not_declared",
                "model catalog references an undeclared operation",
            ));
        }
        if catalog.models.len() > 1024
            || (catalog.policy == ModelCatalogPolicy::Allowlist && catalog.models.is_empty())
        {
            return Err(LlmConfigError::new(
                "invalid_model_catalog",
                "allowlists must be non-empty and catalogs are bounded to 1024 models",
            ));
        }
        normalize_models(&mut catalog.models)?;
    }
    spec.model_catalogs
        .sort_by_key(|catalog| operation_sort_key(catalog.operation_key).unwrap_or_default());
    if spec
        .model_catalogs
        .windows(2)
        .any(|pair| pair[0].operation_key == pair[1].operation_key)
    {
        return Err(LlmConfigError::new(
            "duplicate_model_catalog",
            "there may be only one model catalog per operation",
        ));
    }
    Ok(())
}

fn normalize_models(models: &mut [super::ChannelModel]) -> LlmConfigResult<()> {
    for model in models.iter_mut() {
        model.id = model.id.trim().to_owned();
        model.name = model
            .name
            .take()
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if model.id.is_empty() || model.id.len() > 256 {
            return Err(LlmConfigError::new(
                "invalid_model_id",
                "model id must contain 1..=256 bytes",
            ));
        }
        if model.context_window == Some(0) || model.max_output_tokens == Some(0) {
            return Err(LlmConfigError::new(
                "invalid_model_limits",
                "model token limits must be positive when present",
            ));
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    if models.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(LlmConfigError::new(
            "duplicate_model_id",
            "model ids must be unique within a catalog",
        ));
    }
    Ok(())
}

fn normalize_capabilities(spec: &mut LlmChannelRevisionSpec) -> LlmConfigResult<()> {
    if spec.capabilities.len() > 128 {
        return Err(LlmConfigError::new(
            "channel_capability_limit",
            "channel has too many capabilities",
        ));
    }
    for capability in &mut spec.capabilities {
        if !spec.operation_keys.contains(&capability.operation_key()) {
            return Err(LlmConfigError::new(
                "capability_operation_not_declared",
                "channel capability references an undeclared operation",
            ));
        }
        let hosted_kind = capability.hosted_kind_mut();
        *hosted_kind = hosted_kind.trim().to_owned();
        if hosted_kind.is_empty() || hosted_kind.len() > 128 {
            return Err(LlmConfigError::new(
                "invalid_hosted_capability",
                "hosted kind must contain 1..=128 bytes",
            ));
        }
    }
    let mut keyed = spec
        .capabilities
        .drain(..)
        .map(|capability| {
            canonical::to_string(&capability)
                .map(|key| (key, capability))
                .map_err(|error| {
                    LlmConfigError::new("invalid_channel_capability", error.to_string())
                })
        })
        .collect::<LlmConfigResult<Vec<_>>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    if keyed.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(LlmConfigError::new(
            "duplicate_channel_capability",
            "channel capabilities must be unique",
        ));
    }
    spec.capabilities = keyed
        .into_iter()
        .map(|(_, capability)| capability)
        .collect();
    Ok(())
}
