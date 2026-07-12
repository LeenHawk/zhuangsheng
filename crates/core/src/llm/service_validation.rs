use std::collections::HashSet;

use gproxy_protocol::{Operation, OperationGroup, OperationKind, Provider};

use super::{
    CompactOperationPlan, EmbeddingOperationPlan, ImageOperationPlan, LlmChannelRevision,
    LlmConfigError, LlmConfigResult, LlmNodeModelRef, LlmOperationExecutionPin, ModelCatalogPolicy,
    PreparedCompactOperation, PreparedEmbeddingOperation, PreparedImageOperation,
};

pub fn is_supported_image_key(key: super::OperationKey) -> bool {
    key.operation == Operation::CreateImage && key.kind == OperationKind::Provider(Provider::OpenAi)
}

pub fn is_supported_embedding_key(key: super::OperationKey) -> bool {
    key.operation == Operation::CreateEmbedding
        && matches!(
            key.kind,
            OperationKind::Provider(Provider::OpenAi | Provider::Gemini)
        )
}

pub fn is_supported_compact_key(key: super::OperationKey) -> bool {
    key.operation == Operation::CompactContent
        && key.kind == OperationKind::Provider(Provider::OpenAi)
}

pub fn resolve_service_operation(
    revision: &LlmChannelRevision,
    model: &LlmNodeModelRef,
    group: OperationGroup,
) -> LlmConfigResult<LlmOperationExecutionPin> {
    if model.channel_id != revision.channel_id
        || model.model_id.trim().is_empty()
        || model.operation_key.operation.group() != group
        || !supported_for_group(model.operation_key, group)
        || !revision.spec.operation_keys.contains(&model.operation_key)
    {
        return Err(error(
            "service_operation_not_allowed",
            "service model does not match a declared supported operation",
        ));
    }
    let catalog = revision
        .spec
        .model_catalogs
        .iter()
        .find(|catalog| catalog.operation_key == model.operation_key)
        .ok_or_else(|| error("model_catalog_missing", "service model catalog is missing"))?;
    if catalog.policy == ModelCatalogPolicy::Allowlist
        && !catalog.models.iter().any(|item| item.id == model.model_id)
    {
        return Err(error(
            "model_not_allowed",
            "service model is absent from the channel allowlist",
        ));
    }
    Ok(LlmOperationExecutionPin {
        channel_revision_id: revision.id.clone(),
        model_id: model.model_id.clone(),
        operation_key: model.operation_key,
        operation_taxonomy_version: revision.spec.operation_taxonomy_version,
        adapter_decoder_version: revision.spec.adapter_decoder_version,
    })
}

pub fn validate_image_plan(plan: &ImageOperationPlan) -> LlmConfigResult<()> {
    if !is_supported_image_key(plan.model.operation_key)
        || !valid_ref(&plan.prompt_ref)
        || plan.max_images == 0
        || plan.max_images > 16
        || plan.max_total_bytes == 0
        || plan.max_total_bytes > 256 * 1024 * 1024
        || plan.options.len() > 32
        || plan.options.iter().any(|(key, value)| {
            !valid_option_key(key)
                || crate::canonical::to_vec(value).is_ok_and(|bytes| bytes.len() > 4096)
        })
    {
        return Err(error(
            "invalid_image_plan",
            "image operation plan is invalid",
        ));
    }
    Ok(())
}

pub fn prepare_image_operation(
    revision: &LlmChannelRevision,
    plan: ImageOperationPlan,
) -> LlmConfigResult<PreparedImageOperation> {
    validate_image_plan(&plan)?;
    let operation = resolve_service_operation(revision, &plan.model, OperationGroup::Images)?;
    Ok(PreparedImageOperation { operation, plan })
}

pub fn validate_embedding_plan(plan: &EmbeddingOperationPlan) -> LlmConfigResult<()> {
    let mut sources = HashSet::new();
    if !is_supported_embedding_key(plan.model.operation_key)
        || plan.inputs.is_empty()
        || plan.inputs.len() > 2048
        || plan
            .dimensions
            .is_some_and(|value| value == 0 || value > 65_536)
        || plan.inputs.iter().any(|input| {
            !valid_ref(&input.source_ref)
                || !valid_hash(&input.content_hash)
                || !sources.insert(input.source_ref.as_str())
        })
    {
        return Err(error(
            "invalid_embedding_plan",
            "embedding operation plan is invalid",
        ));
    }
    Ok(())
}

pub fn prepare_embedding_operation(
    revision: &LlmChannelRevision,
    plan: EmbeddingOperationPlan,
) -> LlmConfigResult<PreparedEmbeddingOperation> {
    validate_embedding_plan(&plan)?;
    let operation = resolve_service_operation(revision, &plan.model, OperationGroup::Embeddings)?;
    Ok(PreparedEmbeddingOperation { operation, plan })
}

pub fn validate_compact_plan(plan: &CompactOperationPlan) -> LlmConfigResult<()> {
    let mut sources = HashSet::new();
    if !is_supported_compact_key(plan.model.operation_key)
        || plan.input_refs.is_empty()
        || plan.input_refs.len() > 1024
        || plan
            .input_refs
            .iter()
            .any(|value| !valid_ref(value) || !sources.insert(value.as_str()))
        || plan.target_tokens == 0
        || plan.target_tokens > 1_000_000
    {
        return Err(error(
            "invalid_compact_plan",
            "compact operation plan is invalid",
        ));
    }
    Ok(())
}

pub fn prepare_compact_operation(
    revision: &LlmChannelRevision,
    plan: CompactOperationPlan,
) -> LlmConfigResult<PreparedCompactOperation> {
    validate_compact_plan(&plan)?;
    let operation = resolve_service_operation(revision, &plan.model, OperationGroup::Compact)?;
    Ok(PreparedCompactOperation { operation, plan })
}

fn supported_for_group(key: super::OperationKey, group: OperationGroup) -> bool {
    match group {
        OperationGroup::Images => is_supported_image_key(key),
        OperationGroup::Embeddings => is_supported_embedding_key(key),
        OperationGroup::Compact => is_supported_compact_key(key),
        _ => false,
    }
}

fn valid_ref(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512
}

fn valid_hash(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_option_key(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && !["key", "token", "secret", "credential", "signature"]
            .iter()
            .any(|needle| normalized.contains(needle))
}

fn error(code: &'static str, message: &str) -> LlmConfigError {
    LlmConfigError::new(code, message)
}
