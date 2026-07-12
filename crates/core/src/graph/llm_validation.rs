use std::collections::{HashMap, HashSet};

use crate::{
    ValidationIssue, canonical,
    llm::{
        ChannelCapability, LlmChannelRevision, ModelCapabilityRequirements, Provider,
        ResolvedToolDescriptor,
        context::{
            ContextAssemblyConfig, ContextNormalizationPolicy, ContextPresetVersion,
            normalize_context_spec,
        },
        validate_generation_model, validate_tool_grant,
    },
};

use super::{
    DraftNodeKind, GraphNode, LlmFinalText, LlmNodeLimits, LlmNodeStreaming, LlmOutputSpec,
    LlmRequestOptions, ProviderExtensionsIr, StreamingAudience,
};

#[derive(Debug, Clone, Default)]
pub struct GraphApplyDependencies {
    pub channel_heads: HashMap<String, LlmChannelRevision>,
    pub preset_heads: HashMap<String, ContextPresetVersion>,
    pub tool_descriptors: HashMap<(String, String), ResolvedToolDescriptor>,
}

pub(super) fn normalize_llm_node(
    node: &mut GraphNode,
    dependencies: &GraphApplyDependencies,
    taxonomy: u32,
    decoder: u32,
    issues: &mut Vec<ValidationIssue>,
) {
    let DraftNodeKind::Llm { config } = &mut node.kind else {
        return;
    };
    config.model.model_id = config.model.model_id.trim().to_owned();
    config.model.model_name = config
        .model
        .model_name
        .take()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    let Some(channel) = dependencies.channel_heads.get(&config.model.channel_id) else {
        issues.push(issue(
            "llm_channel_not_published",
            &node.id,
            "model channel has no resolvable head revision",
        ));
        return;
    };
    if channel.spec.operation_taxonomy_version != taxonomy
        || channel.spec.adapter_decoder_version != decoder
    {
        issues.push(issue(
            "operation_version_mismatch",
            &node.id,
            "graph and channel operation versions must match",
        ));
    }
    normalize_context(&mut config.context, dependencies, &node.id, issues);
    normalize_output(config);
    normalize_streaming(config);
    normalize_limits(config, channel, &node.id, issues);
    super::llm_memory_validation::validate_llm_memory(config, &node.id, issues);
    validate_request(config, &node.id, issues);
    validate_tools(config, channel, dependencies, &node.id, issues);
    let requirements = llm_model_requirements(config);
    if let Err(error) = validate_generation_model(
        &channel.spec,
        &config.model,
        &requirements,
        &config.capability_overrides,
    ) {
        issues.push(issue(error.code, &node.id, &error.message));
    }
    validate_ports(node, issues);
}

pub fn llm_model_requirements(config: &super::LlmNodeConfig) -> ModelCapabilityRequirements {
    ModelCapabilityRequirements {
        streaming: config.streaming.as_ref().is_some_and(|value| value.enabled),
        tool_calling: !config.tools.is_empty() || !config.hosted_tools.is_empty(),
        structured_output: matches!(config.output, Some(LlmOutputSpec::Json { .. })),
        vision_input: false,
    }
}

fn normalize_context(
    context: &mut ContextAssemblyConfig,
    dependencies: &GraphApplyDependencies,
    node_id: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match context {
        ContextAssemblyConfig::Preset { preset_id } => {
            *preset_id = preset_id.trim().to_owned();
            if !dependencies.preset_heads.contains_key(preset_id) {
                issues.push(issue(
                    "context_preset_not_published",
                    node_id,
                    "context preset has no resolvable head version",
                ));
            }
        }
        ContextAssemblyConfig::Inline { spec } => {
            match normalize_context_spec(spec.clone(), &ContextNormalizationPolicy::default()) {
                Ok(normalized) => *spec = normalized,
                Err(error) => issues.push(issue(error.code, node_id, &error.message)),
            }
        }
    }
}

fn normalize_output(config: &mut super::LlmNodeConfig) {
    let output = config.output.get_or_insert(LlmOutputSpec::Text {
        final_text: Some(LlmFinalText::LastAssistantTurn),
        allow_empty: false,
    });
    if let LlmOutputSpec::Text { final_text, .. } = output {
        final_text.get_or_insert(LlmFinalText::LastAssistantTurn);
    }
}

fn normalize_streaming(config: &mut super::LlmNodeConfig) {
    config.streaming.get_or_insert(LlmNodeStreaming {
        enabled: false,
        audience: StreamingAudience::Internal,
        persist_chunks: false,
    });
    config
        .request
        .get_or_insert_with(LlmRequestOptions::default);
}

fn normalize_limits(
    config: &mut super::LlmNodeConfig,
    channel: &LlmChannelRevision,
    node_id: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let model = channel
        .spec
        .model_catalogs
        .iter()
        .find(|catalog| catalog.operation_key == config.model.operation_key)
        .and_then(|catalog| {
            catalog
                .models
                .iter()
                .find(|model| model.id == config.model.model_id)
        });
    let limits = config.limits.get_or_insert_with(LlmNodeLimits::default);
    limits.max_model_calls.get_or_insert(8);
    limits.max_count_calls.get_or_insert(2);
    limits.max_tool_calls.get_or_insert(32);
    limits.max_output_repairs.get_or_insert(1);
    limits.max_concurrent_tools.get_or_insert(4);
    limits.max_input_tokens.get_or_insert_with(|| {
        model
            .and_then(|model| model.context_window)
            .unwrap_or(32_768)
    });
    limits.max_output_tokens.get_or_insert_with(|| {
        model
            .and_then(|model| model.max_output_tokens)
            .unwrap_or(4_096)
    });
    let bounded = [
        (limits.max_model_calls, 64),
        (limits.max_count_calls, 16),
        (limits.max_tool_calls, 256),
        (limits.max_output_repairs, 8),
        (limits.max_concurrent_tools, 16),
        (limits.max_input_tokens, 2_000_000),
        (limits.max_output_tokens, 256_000),
    ];
    if bounded
        .into_iter()
        .any(|(value, max)| value.is_none_or(|value| value == 0 || value > max))
        || limits.max_concurrent_tools > limits.max_tool_calls
    {
        issues.push(issue(
            "invalid_llm_limits",
            node_id,
            "LLM limits must be positive and within service hard bounds",
        ));
    }
    if model.is_some_and(|model| {
        model
            .context_window
            .zip(limits.max_input_tokens)
            .is_some_and(|(model, limit)| limit > model)
            || model
                .max_output_tokens
                .zip(limits.max_output_tokens)
                .is_some_and(|(model, limit)| limit > model)
    }) {
        issues.push(issue(
            "llm_limits_exceed_model",
            node_id,
            "LLM hard token limits exceed channel model metadata",
        ));
    }
}

fn validate_request(
    config: &super::LlmNodeConfig,
    node_id: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(request) = &config.request else {
        return;
    };
    if let Some(generation) = &request.generation {
        let invalid_number = generation
            .temperature
            .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
            || generation
                .top_p
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value));
        if invalid_number
            || generation.stop.len() > 16
            || generation
                .stop
                .iter()
                .any(|value| value.is_empty() || value.len() > 1024)
        {
            issues.push(issue(
                "invalid_generation_options",
                node_id,
                "generation options exceed supported bounds",
            ));
        }
        if generation
            .max_output_tokens
            .zip(
                config
                    .limits
                    .as_ref()
                    .and_then(|limits| limits.max_output_tokens),
            )
            .is_some_and(|(preference, hard)| preference == 0 || preference > hard)
        {
            issues.push(issue(
                "generation_output_limit_exceeded",
                node_id,
                "generation max output tokens exceed the node hard limit",
            ));
        }
    }
    if let Some(extensions) = &request.extensions {
        validate_extensions(
            extensions,
            config.model.operation_key.provider_family(),
            node_id,
            issues,
        );
    }
}

fn validate_extensions(
    extensions: &ProviderExtensionsIr,
    provider: Provider,
    node_id: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let entries = [
        (Provider::OpenAi, extensions.openai.as_ref()),
        (Provider::Claude, extensions.claude.as_ref()),
        (Provider::Gemini, extensions.gemini.as_ref()),
    ];
    for (family, extension) in entries {
        let Some(extension) = extension else { continue };
        if family != provider
            || extension
                .extra_headers
                .iter()
                .any(|(name, _)| sensitive_header(name))
            || canonical::to_vec(extension).is_err()
            || canonical::to_vec(extension).is_ok_and(|bytes| bytes.len() > 64 * 1024)
        {
            issues.push(issue(
                "invalid_provider_extension",
                node_id,
                "provider extension is mismatched, sensitive, or too large",
            ));
        }
    }
}

fn sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "x-goog-api-key"
            | "host"
            | "content-length"
            | "transfer-encoding"
    ) || ["token", "secret", "credential", "signature"]
        .iter()
        .any(|needle| name.contains(needle))
}

fn validate_tools(
    config: &super::LlmNodeConfig,
    channel: &LlmChannelRevision,
    dependencies: &GraphApplyDependencies,
    node_id: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut bindings = HashSet::new();
    let mut names = HashSet::new();
    for grant in &config.tools {
        let resolved = dependencies
            .tool_descriptors
            .get(&(grant.tool_id.clone(), grant.version.clone()));
        if resolved.is_none() {
            issues.push(issue(
                "tool_descriptor_not_registered",
                node_id,
                "tool grant has no enabled registered descriptor",
            ));
        }
        let name = grant.exposed_name.as_deref().unwrap_or_else(|| {
            resolved.map_or(grant.tool_id.as_str(), |tool| tool.descriptor.name.as_str())
        });
        if !valid_name(&grant.binding_id)
            || !valid_name(&grant.tool_id)
            || !valid_name(&grant.version)
            || !valid_name(name)
            || !bindings.insert(&grant.binding_id)
            || !names.insert(name)
            || grant.artifact.max_objects == 0
            || grant.artifact.max_bytes == 0
            || grant.failure_policy.as_ref().is_some_and(|policy| {
                policy.max_attempts == 0
                    || policy.max_attempts > 32
                    || policy.retry_backoff_ms.len() as u64 > policy.max_attempts.saturating_sub(1)
                    || policy
                        .retry_backoff_ms
                        .iter()
                        .any(|delay| *delay > 86_400_000)
            })
        {
            issues.push(issue(
                "invalid_tool_grant",
                node_id,
                "tool grant is invalid or duplicated",
            ));
        }
        if let Some(resolved) = resolved
            && let Err(error) = validate_tool_grant(grant, resolved)
        {
            issues.push(issue(error.code, node_id, &error.message));
        }
    }
    for hosted in &config.hosted_tools {
        let supported = channel.spec.capabilities.iter().any(|capability| {
            matches!(capability, ChannelCapability::HostedTool { operation_key, hosted_kind }
                if operation_key == &hosted.operation_key && hosted_kind == &hosted.hosted_kind)
        });
        if !valid_name(&hosted.binding_id)
            || !valid_name(&hosted.hosted_kind)
            || !bindings.insert(&hosted.binding_id)
            || hosted.max_uses_per_model_call == 0
            || !supported
        {
            issues.push(issue(
                "invalid_hosted_tool_binding",
                node_id,
                "hosted tool is invalid, duplicated, or absent from the channel revision",
            ));
        }
    }
}

fn validate_ports(node: &GraphNode, issues: &mut Vec<ValidationIssue>) {
    if node.outputs.len() != 1 || node.outputs[0].name != "default" {
        issues.push(issue(
            "llm_output_port_contract",
            &node.id,
            "phase-one LLMNode must have exactly one default output",
        ));
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn issue(code: &'static str, node_id: &str, message: &str) -> ValidationIssue {
    ValidationIssue::error(code, format!("/nodes/{node_id}"), message)
}
