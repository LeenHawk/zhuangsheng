use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{
    graph::{HostedToolBinding, LlmNodeExecutionSnapshot, LlmOutputSpec},
    llm::{ContentGenerationKind, OperationKind},
};

use super::{
    ir::{HostedToolDescriptorIr, MetadataValue},
    request_builder::{LlmRequestBuildError, ResolvedHostedTool},
};

const PUBLIC_INTERNET_SCOPE: &str = "internet:public";

pub(super) fn resolve_hosted_tools(
    execution: &LlmNodeExecutionSnapshot,
    approved: &BTreeSet<String>,
) -> Result<(Vec<HostedToolDescriptorIr>, Vec<ResolvedHostedTool>), LlmRequestBuildError> {
    let mut kinds = BTreeSet::new();
    let mut descriptors = Vec::with_capacity(execution.hosted_tools.len());
    let mut resolved = Vec::with_capacity(execution.hosted_tools.len());
    for binding in &execution.hosted_tools {
        validate_binding(execution, binding, approved, &mut kinds)?;
        let config = validate_config(binding)?;
        descriptors.push(HostedToolDescriptorIr {
            binding_id: binding.binding_id.clone(),
            hosted_kind: binding.hosted_kind.clone(),
            config,
        });
        resolved.push(ResolvedHostedTool {
            binding: binding.clone(),
            provider_item_kind: "web_search_call".into(),
        });
    }
    Ok((descriptors, resolved))
}

fn validate_binding(
    execution: &LlmNodeExecutionSnapshot,
    binding: &HostedToolBinding,
    approved: &BTreeSet<String>,
    kinds: &mut BTreeSet<String>,
) -> Result<(), LlmRequestBuildError> {
    if binding.operation_key != execution.operation.operation_key {
        return error(
            "hosted_tool_operation_mismatch",
            "hosted tool operation does not match the pinned generation operation",
        );
    }
    if binding.effect.requires_approval && !approved.contains(&binding.binding_id) {
        return error(
            "hosted_tool_approval_required",
            "hosted tool capability envelope requires approval before exposure",
        );
    }
    let supported_shape = matches!(
        binding.operation_key.kind,
        OperationKind::ContentGeneration(ContentGenerationKind::OpenAiResponses)
    );
    if !supported_shape || binding.hosted_kind != "web_search" {
        return error(
            "unsupported_hosted_tool",
            "the pinned adapter has no allowlisted mapping for this hosted tool",
        );
    }
    if !execution.tools.is_empty() || !matches!(execution.output, Some(LlmOutputSpec::Text { .. }))
    {
        return error(
            "unsupported_hosted_tool_loop",
            "phase-one hosted web search requires a text-only call without local tools",
        );
    }
    if !kinds.insert(binding.hosted_kind.clone()) {
        return error(
            "ambiguous_hosted_tool_binding",
            "provider hosted item kinds must map to exactly one binding",
        );
    }
    if binding.max_uses_per_model_call == 0
        || binding.max_uses_per_model_call > 64
        || binding.effect.operation_key.trim().is_empty()
        || binding.effect.operation_key.len() > 128
        || binding.resource_scopes.as_slice() != [PUBLIC_INTERNET_SCOPE]
    {
        return error(
            "invalid_hosted_tool_policy",
            "hosted tool limits, effect operation, or resource scope are invalid",
        );
    }
    Ok(())
}

fn validate_config(
    binding: &HostedToolBinding,
) -> Result<BTreeMap<String, MetadataValue>, LlmRequestBuildError> {
    let mut config = BTreeMap::new();
    for (key, value) in &binding.model_facing_config {
        if sensitive_name(key) {
            return error(
                "hosted_tool_config_unsafe",
                "hosted tool config contains a sensitive field name",
            );
        }
        if key != "search_context_size" {
            return error(
                "unsupported_hosted_tool_config",
                "hosted tool config contains a field outside the adapter allowlist",
            );
        }
        let Value::String(value) = value else {
            return error(
                "hosted_tool_config_invalid",
                "search context size must be a string",
            );
        };
        if !matches!(value.as_str(), "low" | "medium" | "high") {
            return error(
                "hosted_tool_config_invalid",
                "search context size must be low, medium, or high",
            );
        }
        config.insert(key.clone(), MetadataValue::String(value.clone()));
    }
    Ok(config)
}

fn sensitive_name(value: &str) -> bool {
    let name = value.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "host"
            | "path"
            | "url"
    ) || ["token", "secret", "credential", "signature", "header"]
        .iter()
        .any(|needle| name.contains(needle))
}

fn error<T>(code: &'static str, message: &'static str) -> Result<T, LlmRequestBuildError> {
    Err(LlmRequestBuildError::new(code, message))
}
