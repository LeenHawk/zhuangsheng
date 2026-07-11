use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::{
    canonical,
    graph::ToolChoiceIr,
    llm::{
        LlmOperationExecutionPin,
        ir::{
            LlmContentPartIr, LlmFinishReason, LlmRequestIr, LlmResponseIr, LlmTurnItemIr,
            LlmUsageIr, MessageRole, ResponseFormatIr, ToolCallIr,
        },
    },
};

use super::{
    AdapterExecutionOptions, AdapterResources, DecodedTerminalDraft, OpaqueAttachmentDraft,
    OpaqueAttachmentTarget, SensitiveEntryDraft, ShapeAdapterError, ShapeAdapterKey,
    WireGenerationRequest,
    common::{
        apply_extension_fields, finish_request, opaque_value, openai_content, parse_typed_terminal,
        prepare, text_only,
    },
};

pub fn encode_openai_chat_request(
    pin: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    resources: &AdapterResources,
    options: AdapterExecutionOptions,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    let target = prepare(
        pin,
        request,
        options,
        ShapeAdapterKey::OpenAiChatCompletionsV1,
    )?;
    if request.continuation.is_some() || !request.hosted_tools.is_empty() {
        return Err(ShapeAdapterError::new(
            "unsupported_chat_capability",
            "chat completions v1 does not accept top-level continuation or hosted tools",
        ));
    }
    let mut messages = Vec::new();
    for instruction in &request.instructions {
        let role = match instruction.role {
            crate::llm::ir::InstructionRole::Policy | crate::llm::ir::InstructionRole::System => {
                "system"
            }
            crate::llm::ir::InstructionRole::Developer => "developer",
            crate::llm::ir::InstructionRole::Context => "user",
        };
        messages.push(json!({"role":role,"content":text_only(&instruction.content)?}));
    }
    let mut call_ids = HashMap::new();
    for item in &request.transcript {
        match item {
            LlmTurnItemIr::Message { role, content, .. } => match role {
                MessageRole::User => messages.push(json!({
                    "role":"user",
                    "content":openai_content(content, resources)?,
                })),
                MessageRole::Assistant => messages.push(json!({
                    "role":"assistant",
                    "content":text_only(content)?,
                })),
            },
            LlmTurnItemIr::AssistantToolCall { call, .. } => {
                let provider_id = call.provider_call_id.as_deref().unwrap_or(&call.id);
                call_ids.insert(call.id.as_str(), provider_id.to_owned());
                messages.push(json!({
                    "role":"assistant",
                    "content":null,
                    "tool_calls":[{
                        "type":"function",
                        "id":provider_id,
                        "function":{
                            "name":call.name,
                            "arguments":canonical::to_string(&call.arguments).map_err(|_| ShapeAdapterError::new("invalid_tool_arguments","tool arguments cannot be serialized"))?
                        }
                    }]
                }));
            }
            LlmTurnItemIr::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                let provider_id = call_ids.get(tool_call_id.as_str()).ok_or_else(|| {
                    ShapeAdapterError::new(
                        "tool_result_call_missing",
                        "tool result cannot recover provider call id",
                    )
                })?;
                messages.push(json!({
                    "role":"tool",
                    "tool_call_id":provider_id,
                    "content":text_only(content)?,
                }));
            }
            LlmTurnItemIr::HostedTool {
                opaque_item_ref: Some(reference),
                ..
            }
            | LlmTurnItemIr::Reasoning {
                opaque_item_ref: Some(reference),
                ..
            } => messages.push(opaque_value(reference, resources)?),
            LlmTurnItemIr::HostedTool { .. } | LlmTurnItemIr::Reasoning { .. } => {
                return Err(ShapeAdapterError::new(
                    "opaque_roundtrip_material_missing",
                    "chat hosted/reasoning item has no same-shape opaque material",
                ));
            }
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), Value::Bool(options.stream));
    if options.stream {
        body.insert("stream_options".into(), json!({"include_usage":true}));
    }
    body.insert(
        "max_completion_tokens".into(),
        Value::Number(u32_limit(options.max_output_tokens)?.into()),
    );
    apply_generation(&mut body, request, options.max_output_tokens)?;
    apply_tools(&mut body, request);
    apply_response_format(&mut body, request);
    if let Some(extension) = request
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.openai.as_ref())
    {
        apply_extension_fields(
            &mut body,
            extension,
            &[
                "parallel_tool_calls",
                "store",
                "reasoning_effort",
                "verbosity",
            ],
        )?;
    }
    finish_request::<gproxy_protocol::openai::ChatCompletionRequest>(
        ShapeAdapterKey::OpenAiChatCompletionsV1,
        pin,
        target,
        Value::Object(body),
    )
}

pub fn decode_openai_chat_terminal(
    pin: &LlmOperationExecutionPin,
    model_call_id: &str,
    bytes: &[u8],
) -> Result<DecodedTerminalDraft, ShapeAdapterError> {
    let descriptor = super::resolve_shape_adapter(pin)?;
    if descriptor.key != ShapeAdapterKey::OpenAiChatCompletionsV1 {
        return Err(ShapeAdapterError::new(
            "adapter_execution_mismatch",
            "chat terminal decoder does not match execution pin",
        ));
    }
    let value = parse_typed_terminal::<gproxy_protocol::openai::ChatCompletionResponse>(bytes)?;
    let choices = value
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ShapeAdapterError::new("chat_choices_missing", "chat choices are missing")
        })?;
    if choices.len() != 1 {
        return Err(ShapeAdapterError::new(
            "chat_choice_cardinality",
            "chat adapter requires exactly one choice",
        ));
    }
    let choice = &choices[0];
    let message = choice
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ShapeAdapterError::new(
                "chat_message_missing",
                "chat choice has no assistant message",
            )
        })?;
    let assistant_text = message
        .get("content")
        .and_then(Value::as_str)
        .or_else(|| message.get("refusal").and_then(Value::as_str))
        .filter(|text| !text.is_empty());
    let mut items = Vec::new();
    let mut sensitive_entries = Vec::new();
    let mut opaque_attachments = Vec::new();
    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let id = format!("{model_call_id}:reasoning:0");
        let entry_key = "chat_reasoning_0".to_owned();
        items.push(LlmTurnItemIr::Reasoning {
            id: id.clone(),
            summary: None,
            opaque_item_ref: None,
        });
        sensitive_entries.push(SensitiveEntryDraft {
            entry_key: entry_key.clone(),
            adapter_key: ShapeAdapterKey::OpenAiChatCompletionsV1,
            semantic_slot: "reasoning_content".into(),
            opaque_bytes: canonical::to_vec(&json!({
                "role":"assistant",
                "content":null,
                "reasoning_content":reasoning
            }))
            .map_err(|_| {
                ShapeAdapterError::new(
                    "opaque_item_encode_failed",
                    "reasoning sidecar could not be encoded",
                )
            })?,
        });
        opaque_attachments.push(OpaqueAttachmentDraft {
            entry_key,
            target: OpaqueAttachmentTarget::Item { item_id: id },
        });
    }
    if let Some(text) = assistant_text {
        items.push(LlmTurnItemIr::Message {
            id: format!("{model_call_id}:message:0"),
            role: MessageRole::Assistant,
            content: vec![LlmContentPartIr::Text { text: text.into() }],
            provenance: None,
        });
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (index, tool) in tool_calls.iter().enumerate() {
            let provider_id = required_string(tool, "id", "chat_tool_call_id_missing")?;
            let function = tool.get("function").ok_or_else(|| {
                ShapeAdapterError::new(
                    "chat_tool_function_missing",
                    "chat tool call has no function",
                )
            })?;
            let name = required_string(function, "name", "chat_tool_name_missing")?;
            let raw_arguments =
                required_string(function, "arguments", "chat_tool_arguments_missing")?;
            let arguments = serde_json::from_str(raw_arguments).map_err(|_| {
                ShapeAdapterError::new(
                    "chat_tool_arguments_invalid",
                    "chat tool arguments are not complete JSON",
                )
            })?;
            items.push(LlmTurnItemIr::AssistantToolCall {
                id: format!("{model_call_id}:tool:{index}"),
                call: ToolCallIr {
                    id: format!("{model_call_id}:call:{index}"),
                    provider_call_id: Some(provider_id.into()),
                    name: name.into(),
                    arguments,
                },
            });
        }
    }
    let response = LlmResponseIr {
        model_call_id: model_call_id.into(),
        items,
        usage: decode_usage(value.get("usage")),
        finish_reason: Some(decode_finish_reason(
            choice.get("finish_reason").and_then(Value::as_str),
        )),
        continuation: None,
        raw_response_ref: None,
    };
    crate::llm::ir::validate_response_ir(&response)
        .map_err(|error| ShapeAdapterError::new(error.code, error.message))?;
    Ok(DecodedTerminalDraft {
        response,
        sensitive_entries,
        opaque_attachments,
    })
}

fn apply_generation(
    body: &mut Map<String, Value>,
    request: &LlmRequestIr,
    hard_output_limit: u64,
) -> Result<(), ShapeAdapterError> {
    let Some(generation) = &request.generation else {
        return Ok(());
    };
    if let Some(value) = generation.temperature {
        body.insert("temperature".into(), json!(value));
    }
    if let Some(value) = generation.top_p {
        body.insert("top_p".into(), json!(value));
    }
    if let Some(value) = generation.seed {
        body.insert("seed".into(), json!(value));
    }
    if !generation.stop.is_empty() {
        body.insert("stop".into(), json!(generation.stop));
    }
    if let Some(value) = generation.max_output_tokens {
        body.insert(
            "max_completion_tokens".into(),
            json!(u32_limit(value.min(hard_output_limit))?),
        );
    }
    Ok(())
}

fn apply_tools(body: &mut Map<String, Value>, request: &LlmRequestIr) {
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type":"function",
                            "function":{
                                "name":tool.name,
                                "description":tool.description,
                                "parameters":tool.input_schema.document,
                                "strict":true
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if let Some(choice) = &request.tool_choice {
        body.insert(
            "tool_choice".into(),
            match choice {
                ToolChoiceIr::Auto => json!("auto"),
                ToolChoiceIr::None => json!("none"),
                ToolChoiceIr::Required => json!("required"),
                ToolChoiceIr::Named { name } => json!({"type":"function","function":{"name":name}}),
            },
        );
    }
}

fn apply_response_format(body: &mut Map<String, Value>, request: &LlmRequestIr) {
    let Some(format) = &request.response_format else {
        return;
    };
    body.insert(
        "response_format".into(),
        match format {
            ResponseFormatIr::Text => json!({"type":"text"}),
            ResponseFormatIr::Json { schema: None, .. } => json!({"type":"json_object"}),
            ResponseFormatIr::Json {
                schema: Some(schema),
                strict,
            } => json!({
                "type":"json_schema",
                "json_schema":{"name":"response","schema":schema.document,"strict":strict}
            }),
        },
    );
}

fn required_string<'a>(
    value: &'a Value,
    field: &str,
    code: &'static str,
) -> Result<&'a str, ShapeAdapterError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ShapeAdapterError::new(code, "required chat response field is missing"))
}

fn decode_usage(value: Option<&Value>) -> Option<LlmUsageIr> {
    let value = value?;
    Some(LlmUsageIr {
        input_tokens: value.get("prompt_tokens").and_then(Value::as_u64),
        output_tokens: value.get("completion_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        cached_input_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    })
}

fn decode_finish_reason(value: Option<&str>) -> LlmFinishReason {
    match value {
        Some("stop") => LlmFinishReason::Completed,
        Some("tool_calls" | "function_call") => LlmFinishReason::ToolCalls,
        Some("length") => LlmFinishReason::Length,
        Some("content_filter") => LlmFinishReason::ContentFilter,
        _ => LlmFinishReason::Unknown,
    }
}

fn u32_limit(value: u64) -> Result<u32, ShapeAdapterError> {
    u32::try_from(value).map_err(|_| {
        ShapeAdapterError::new(
            "wire_integer_limit",
            "token limit does not fit provider wire type",
        )
    })
}
