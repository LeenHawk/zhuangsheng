use crate::llm::{LlmOperationExecutionPin, ir::LlmRequestIr};

use super::{
    AdapterExecutionOptions, AdapterResources, DecodedTerminalDraft, ShapeAdapterError,
    ShapeAdapterKey, WireGenerationRequest, decode_claude_terminal, decode_gemini_terminal,
    decode_openai_chat_terminal, decode_openai_responses_terminal, encode_claude_request,
    encode_gemini_request, encode_openai_chat_request, encode_openai_responses_request,
    resolve_shape_adapter,
};

pub fn encode_generation_request(
    pin: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    resources: &AdapterResources,
    options: AdapterExecutionOptions,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    match resolve_shape_adapter(pin)?.key {
        ShapeAdapterKey::OpenAiResponsesV1 => {
            encode_openai_responses_request(pin, request, resources, options)
        }
        ShapeAdapterKey::OpenAiChatCompletionsV1 => {
            encode_openai_chat_request(pin, request, resources, options)
        }
        ShapeAdapterKey::ClaudeMessagesV1 => {
            encode_claude_request(pin, request, resources, options)
        }
        ShapeAdapterKey::GeminiGenerateContentV1 => {
            encode_gemini_request(pin, request, resources, options)
        }
    }
}

pub fn restore_generation_request(
    pin: &LlmOperationExecutionPin,
    body: Vec<u8>,
    stream: bool,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    if body.is_empty() || body.len() > 16 * 1024 * 1024 {
        return Err(ShapeAdapterError::new(
            "wire_request_size_limit",
            "persisted provider request is empty or exceeds 16 MiB",
        ));
    }
    serde_json::from_slice::<serde_json::Value>(&body).map_err(|_| {
        ShapeAdapterError::new(
            "wire_request_restore_failed",
            "persisted provider request is not valid JSON",
        )
    })?;
    let descriptor = resolve_shape_adapter(pin)?;
    let target = gproxy_protocol::request_target(pin.operation_key, &pin.model_id, stream);
    Ok(WireGenerationRequest::from_parts(
        descriptor.key,
        pin.clone(),
        target.method,
        target.path,
        target.query,
        body,
    ))
}

pub fn decode_generation_terminal(
    pin: &LlmOperationExecutionPin,
    model_call_id: &str,
    bytes: &[u8],
) -> Result<DecodedTerminalDraft, ShapeAdapterError> {
    match resolve_shape_adapter(pin)?.key {
        ShapeAdapterKey::OpenAiResponsesV1 => {
            decode_openai_responses_terminal(pin, model_call_id, bytes)
        }
        ShapeAdapterKey::OpenAiChatCompletionsV1 => {
            decode_openai_chat_terminal(pin, model_call_id, bytes)
        }
        ShapeAdapterKey::ClaudeMessagesV1 => decode_claude_terminal(pin, model_call_id, bytes),
        ShapeAdapterKey::GeminiGenerateContentV1 => {
            decode_gemini_terminal(pin, model_call_id, bytes)
        }
    }
}
