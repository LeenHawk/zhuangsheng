use serde_json::{Map, Value, json};

use crate::{
    canonical,
    llm::{LlmOperationExecutionPin, Operation, OperationKey},
};

use super::{ShapeAdapterError, ShapeAdapterKey, WireGenerationRequest};

pub fn encode_count_request(
    generation: &WireGenerationRequest,
    count_operation: OperationKey,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    if count_operation.operation != Operation::CountTokens
        || count_operation.provider_family() != generation.operation.operation_key.provider_family()
    {
        return Err(error(
            "count_operation_mismatch",
            "count operation does not match the generation provider",
        ));
    }
    let generation_body: Value = serde_json::from_slice(generation.body()).map_err(|_| {
        error(
            "count_request_encoding_failed",
            "generation wire body is not valid JSON",
        )
    })?;
    let body = match generation.adapter_key {
        ShapeAdapterKey::OpenAiResponsesV1 => {
            filter_object(generation_body, OPENAI_RESPONSE_COUNT_FIELDS)?
        }
        ShapeAdapterKey::ClaudeMessagesV1 => filter_object(generation_body, CLAUDE_COUNT_FIELDS)?,
        ShapeAdapterKey::GeminiGenerateContentV1 => {
            json!({"generateContentRequest":generation_body})
        }
        ShapeAdapterKey::OpenAiChatCompletionsV1 => {
            return Err(error(
                "count_shape_unsupported",
                "OpenAI chat completions has no wire-equivalent count shape",
            ));
        }
    };
    let pin = LlmOperationExecutionPin {
        channel_revision_id: generation.operation.channel_revision_id.clone(),
        model_id: generation.operation.model_id.clone(),
        operation_key: count_operation,
        operation_taxonomy_version: generation.operation.operation_taxonomy_version,
        adapter_decoder_version: generation.operation.adapter_decoder_version,
    };
    let target = gproxy_protocol::request_target(count_operation, &pin.model_id, false);
    Ok(WireGenerationRequest::from_parts(
        generation.adapter_key,
        pin,
        target.method,
        target.path,
        target.query,
        canonical::to_vec(&body).map_err(|_| {
            error(
                "count_request_encoding_failed",
                "count request cannot be canonically serialized",
            )
        })?,
    ))
}

pub fn decode_count_terminal(
    request: &WireGenerationRequest,
    bytes: &[u8],
) -> Result<u64, ShapeAdapterError> {
    if request.operation.operation_key.operation != Operation::CountTokens {
        return Err(error(
            "count_operation_mismatch",
            "count response does not match a count operation",
        ));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        error(
            "count_response_invalid",
            "provider count response is not valid JSON",
        )
    })?;
    let field = match request.adapter_key {
        ShapeAdapterKey::OpenAiResponsesV1 | ShapeAdapterKey::ClaudeMessagesV1 => "input_tokens",
        ShapeAdapterKey::GeminiGenerateContentV1 => "totalTokens",
        ShapeAdapterKey::OpenAiChatCompletionsV1 => {
            return Err(error(
                "count_shape_unsupported",
                "OpenAI chat completions has no count response shape",
            ));
        }
    };
    value
        .as_object()
        .and_then(|object| object.get(field))
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            error(
                "count_response_invalid",
                "provider count response has no non-negative token count",
            )
        })
}

fn filter_object(value: Value, allowed: &[&str]) -> Result<Value, ShapeAdapterError> {
    let object = value.as_object().ok_or_else(|| {
        error(
            "count_request_encoding_failed",
            "generation wire body is not an object",
        )
    })?;
    let filtered: Map<String, Value> = allowed
        .iter()
        .filter_map(|key| {
            object
                .get(*key)
                .cloned()
                .map(|value| ((*key).into(), value))
        })
        .collect();
    if filtered.is_empty() {
        return Err(error(
            "count_request_encoding_failed",
            "generation wire body has no countable fields",
        ));
    }
    Ok(Value::Object(filtered))
}

fn error(code: &'static str, message: &'static str) -> ShapeAdapterError {
    ShapeAdapterError {
        code,
        message: message.into(),
    }
}

const OPENAI_RESPONSE_COUNT_FIELDS: &[&str] = &[
    "conversation",
    "input",
    "instructions",
    "model",
    "parallel_tool_calls",
    "personality",
    "previous_response_id",
    "reasoning",
    "service_tier",
    "text",
    "tool_choice",
    "tools",
    "truncation",
];

const CLAUDE_COUNT_FIELDS: &[&str] = &[
    "model",
    "messages",
    "cache_control",
    "context_management",
    "diagnostics",
    "mcp_servers",
    "output_config",
    "output_format",
    "service_tier",
    "speed",
    "system",
    "thinking",
    "tool_choice",
    "tools",
];
