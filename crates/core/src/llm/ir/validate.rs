use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::{
    canonical,
    graph::{ProviderExtensionsIr, ToolChoiceIr},
    llm::is_supported_generation_key,
    schema,
};

use super::*;

const MAX_IR_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct IrValidationError {
    pub code: &'static str,
    pub message: String,
}

pub fn validate_request_ir(request: &LlmRequestIr) -> Result<(), IrValidationError> {
    bounded_id(&request.model, 256, "invalid_model_id")?;
    if request.instructions.len() > 256
        || request.transcript.len() > 4096
        || request.tools.len() > 128
        || request.hosted_tools.len() > 64
    {
        return error(
            "llm_ir_collection_limit",
            "LLM request collection limit exceeded",
        );
    }
    let mut ids = HashSet::new();
    for instruction in &request.instructions {
        bounded_id(&instruction.id, 128, "invalid_instruction_id")?;
        if !ids.insert(instruction.id.as_str()) {
            return error(
                "duplicate_ir_item_id",
                "instruction and transcript ids must be unique",
            );
        }
        validate_instruction(instruction)?;
    }
    validate_transcript(&request.transcript, &mut ids)?;
    validate_tool_descriptors(request)?;
    validate_metadata(&request.metadata)?;
    if let Some(continuation) = &request.continuation {
        validate_continuation(continuation, None)?;
    }
    if let Some(format) = &request.response_format
        && let ResponseFormatIr::Json {
            schema: Some(spec), ..
        } = format
    {
        schema::compile(spec).map_err(|error| IrValidationError {
            code: "invalid_response_schema",
            message: error.to_string(),
        })?;
    }
    validate_extensions(request.extensions.as_ref())?;
    bounded_canonical(request)
}

pub fn validate_response_ir(response: &LlmResponseIr) -> Result<(), IrValidationError> {
    bounded_id(&response.model_call_id, 128, "invalid_model_call_id")?;
    if response.items.len() > 4096 {
        return error("llm_ir_collection_limit", "LLM response has too many items");
    }
    let mut ids = HashSet::new();
    validate_transcript(&response.items, &mut ids)?;
    if let Some(continuation) = &response.continuation {
        validate_continuation(continuation, Some(&response.model_call_id))?;
    }
    if let Some(raw) = &response.raw_response_ref {
        raw.validate().map_err(|message| IrValidationError {
            code: "invalid_raw_response_ref",
            message: message.into(),
        })?;
    }
    if let Some(usage) = &response.usage {
        validate_usage(usage)?;
    }
    bounded_canonical(response)
}

fn validate_instruction(instruction: &InstructionIr) -> Result<(), IrValidationError> {
    validate_content(&instruction.content, true)?;
    bounded_id(&instruction.provenance.id, 128, "invalid_provenance_id")?;
    bounded_id(
        &instruction.provenance.item_id,
        128,
        "invalid_provenance_item",
    )?;
    let authority_valid = match instruction.role {
        InstructionRole::Policy => instruction.provenance.trust == ContextTrust::RuntimePolicy,
        InstructionRole::System | InstructionRole::Developer => matches!(
            instruction.provenance.trust,
            ContextTrust::RuntimePolicy | ContextTrust::TrustedConfig
        ),
        InstructionRole::Context => true,
    };
    if !authority_valid {
        return error(
            "instruction_authority_violation",
            "instruction role exceeds provenance trust",
        );
    }
    Ok(())
}

fn validate_transcript<'a>(
    items: &'a [LlmTurnItemIr],
    ids: &mut HashSet<&'a str>,
) -> Result<(), IrValidationError> {
    let mut calls: HashMap<&str, &str> = HashMap::new();
    let mut results = HashSet::new();
    for item in items {
        bounded_id(item.id(), 128, "invalid_ir_item_id")?;
        if !ids.insert(item.id()) {
            return error(
                "duplicate_ir_item_id",
                "instruction and transcript ids must be unique",
            );
        }
        match item {
            LlmTurnItemIr::Message {
                content,
                placeholder,
                ..
            } => {
                if *placeholder {
                    validate_content(content, false)?;
                    if !content.is_empty() {
                        return error(
                            "invalid_message_placeholder",
                            "adapter placeholders must have empty content",
                        );
                    }
                } else {
                    validate_content(content, true)?;
                }
            }
            LlmTurnItemIr::AssistantToolCall { call, .. } => {
                bounded_id(&call.id, 128, "invalid_tool_call_id")?;
                validate_tool_name(&call.name)?;
                if calls.insert(&call.id, &call.name).is_some() {
                    return error("duplicate_tool_call_id", "tool call ids must be unique");
                }
                let bytes =
                    canonical::to_vec(&call.arguments).map_err(|error| IrValidationError {
                        code: "invalid_tool_arguments",
                        message: error.to_string(),
                    })?;
                if bytes.len() > 256 * 1024 {
                    return error("tool_arguments_limit", "tool arguments exceed 256 KiB");
                }
            }
            LlmTurnItemIr::ToolResult {
                tool_call_id,
                tool_name,
                content,
                ..
            } => {
                let Some(expected_name) = calls.get(tool_call_id.as_str()) else {
                    return error(
                        "orphan_tool_result",
                        "tool result must follow its assistant tool call",
                    );
                };
                if *expected_name != tool_name || !results.insert(tool_call_id) {
                    return error(
                        "invalid_tool_result_pair",
                        "tool result name or cardinality does not match its call",
                    );
                }
                validate_content(content, true)?;
            }
            LlmTurnItemIr::HostedTool {
                binding_id,
                kind,
                display_content,
                opaque_item_ref,
                ..
            } => {
                bounded_id(binding_id, 128, "invalid_hosted_binding")?;
                bounded_id(kind, 128, "invalid_hosted_kind")?;
                validate_content(display_content, false)?;
                if let Some(reference) = opaque_item_ref {
                    validate_continuation(reference, None)?;
                }
            }
            LlmTurnItemIr::Reasoning {
                summary,
                opaque_item_ref,
                ..
            } => {
                if summary
                    .as_ref()
                    .is_some_and(|value| value.len() > 16 * 1024)
                {
                    return error(
                        "reasoning_summary_limit",
                        "reasoning summary exceeds 16 KiB",
                    );
                }
                if let Some(reference) = opaque_item_ref {
                    validate_continuation(reference, None)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_content(content: &[LlmContentPartIr], required: bool) -> Result<(), IrValidationError> {
    if content.len() > 256 || required && content.is_empty() {
        return error(
            "content_part_limit",
            "content parts are empty or exceed the limit",
        );
    }
    for part in content {
        match part {
            LlmContentPartIr::Text { text } => {
                if text.is_empty() || text.len() > MAX_TEXT_BYTES {
                    return error(
                        "content_text_limit",
                        "text part is empty or exceeds one MiB",
                    );
                }
            }
            LlmContentPartIr::Image { artifact_ref } => {
                validate_artifact(artifact_ref)?;
                if !artifact_ref.media_type.starts_with("image/") {
                    return error(
                        "invalid_image_media_type",
                        "image part must reference image media",
                    );
                }
            }
            LlmContentPartIr::File { artifact_ref } => validate_artifact(artifact_ref)?,
        }
    }
    Ok(())
}

fn validate_artifact(reference: &crate::artifact::ArtifactRef) -> Result<(), IrValidationError> {
    reference.validate().map_err(|message| IrValidationError {
        code: "invalid_artifact_ref",
        message: message.into(),
    })
}

fn validate_tool_descriptors(request: &LlmRequestIr) -> Result<(), IrValidationError> {
    let mut names = HashSet::new();
    for tool in &request.tools {
        validate_tool_name(&tool.name)?;
        if !names.insert(tool.name.as_str()) {
            return error(
                "duplicate_tool_name",
                "tool names must be unique within a request",
            );
        }
        if tool
            .description
            .as_ref()
            .is_some_and(|value| value.len() > 4096)
        {
            return error(
                "tool_description_limit",
                "tool description exceeds 4096 bytes",
            );
        }
        schema::compile(&tool.input_schema).map_err(|error| IrValidationError {
            code: "invalid_tool_schema",
            message: error.to_string(),
        })?;
    }
    let mut hosted = HashSet::new();
    for tool in &request.hosted_tools {
        bounded_id(&tool.binding_id, 128, "invalid_hosted_binding")?;
        bounded_id(&tool.hosted_kind, 128, "invalid_hosted_kind")?;
        if !hosted.insert(tool.binding_id.as_str()) {
            return error(
                "duplicate_hosted_binding",
                "hosted binding ids must be unique",
            );
        }
    }
    if let Some(ToolChoiceIr::Named { name }) = &request.tool_choice
        && !names.contains(name.as_str())
    {
        return error(
            "unknown_tool_choice",
            "named tool choice is not exposed by this request",
        );
    }
    Ok(())
}

fn validate_metadata(
    metadata: &std::collections::BTreeMap<String, MetadataValue>,
) -> Result<(), IrValidationError> {
    if metadata.len() > 32 {
        return error("metadata_limit", "metadata contains more than 32 entries");
    }
    for (key, value) in metadata {
        if key.is_empty()
            || key.len() > 128
            || sensitive_name(key)
            || matches!(value, MetadataValue::String(value) if value.len() > 4096)
        {
            return error("invalid_metadata", "metadata key or value is unsafe");
        }
    }
    let bytes = canonical::to_vec(metadata).map_err(|error| IrValidationError {
        code: "invalid_metadata",
        message: error.to_string(),
    })?;
    if bytes.len() > 16 * 1024 {
        return error("metadata_limit", "metadata exceeds 16 KiB");
    }
    Ok(())
}

fn validate_continuation(
    reference: &OpaqueContinuationRef,
    expected_call_id: Option<&str>,
) -> Result<(), IrValidationError> {
    if !is_supported_generation_key(reference.operation_key)
        || !crate::compatibility::supports_operation_versions(
            reference.operation_taxonomy_version,
            reference.adapter_decoder_version,
        )
        || expected_call_id.is_some_and(|expected| expected != reference.model_call_id)
    {
        return error(
            "continuation_compatibility_error",
            "opaque continuation operation or version is incompatible",
        );
    }
    for value in [
        &reference.adapter_key,
        &reference.model_call_id,
        &reference.entry_ref.object_id,
        &reference.entry_ref.entry_key,
    ] {
        bounded_id(value, 128, "invalid_continuation_ref")?;
    }
    if reference.digest.len() != 71
        || !reference.digest.starts_with("sha256:")
        || !reference.digest[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return error(
            "invalid_continuation_digest",
            "opaque continuation digest is invalid",
        );
    }
    Ok(())
}

fn validate_usage(usage: &LlmUsageIr) -> Result<(), IrValidationError> {
    let values = [
        usage.input_tokens,
        usage.output_tokens,
        usage.total_tokens,
        usage.cached_input_tokens,
        usage.reasoning_tokens,
    ];
    if values
        .into_iter()
        .flatten()
        .any(|value| value > 10_000_000_000)
    {
        return error("usage_limit", "usage value exceeds the supported bound");
    }
    if usage
        .input_tokens
        .zip(usage.cached_input_tokens)
        .is_some_and(|(input, cached)| cached > input)
    {
        return error("invalid_usage", "cached input tokens exceed input tokens");
    }
    Ok(())
}

fn validate_extensions(extensions: Option<&ProviderExtensionsIr>) -> Result<(), IrValidationError> {
    let Some(extensions) = extensions else {
        return Ok(());
    };
    for extension in [
        extensions.openai.as_ref(),
        extensions.claude.as_ref(),
        extensions.gemini.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if extension
            .extra_headers
            .keys()
            .any(|name| sensitive_name(name))
        {
            return error(
                "sensitive_extension_header",
                "provider extension header is sensitive",
            );
        }
    }
    Ok(())
}

fn validate_tool_name(value: &str) -> Result<(), IrValidationError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return error("invalid_tool_name", "tool name has an unsupported shape");
    }
    Ok(())
}

fn sensitive_name(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "authorization",
        "cookie",
        "api_key",
        "apikey",
        "token",
        "secret",
        "credential",
        "signature",
        "password",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn bounded_id(value: &str, max: usize, code: &'static str) -> Result<(), IrValidationError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return error(
            code,
            "identifier is empty, too long, or contains control characters",
        );
    }
    Ok(())
}

fn bounded_canonical(value: &impl serde::Serialize) -> Result<(), IrValidationError> {
    let bytes = canonical::to_vec(value).map_err(|error| IrValidationError {
        code: "invalid_llm_ir",
        message: error.to_string(),
    })?;
    if bytes.len() > MAX_IR_BYTES {
        return error("llm_ir_size_limit", "LLM IR exceeds 16 MiB");
    }
    Ok(())
}

fn error<T>(code: &'static str, message: &str) -> Result<T, IrValidationError> {
    Err(IrValidationError {
        code,
        message: message.into(),
    })
}
