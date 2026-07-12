use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::{
    canonical,
    graph::ToolChoiceIr,
    llm::{
        LlmOperationExecutionPin,
        ir::{
            InstructionRole, LlmContentPartIr, LlmFinishReason, LlmRequestIr, LlmResponseIr,
            LlmTurnItemIr, LlmUsageIr, MessageRole, ResponseFormatIr, ToolCallIr,
        },
    },
};

use super::{
    AdapterExecutionOptions, AdapterResources, DecodedTerminalDraft, ShapeAdapterError,
    ShapeAdapterKey, WireGenerationRequest,
    common::{
        apply_extension_fields, finish_request, opaque_value, openai_responses_content,
        parse_typed_terminal, prepare, required_string, text_only,
    },
    openai_responses_opaque::{push_opaque_hosted, push_opaque_reasoning},
};

pub fn encode_openai_responses_request(
    pin: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    resources: &AdapterResources,
    options: AdapterExecutionOptions,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    let target = prepare(pin, request, options, ShapeAdapterKey::OpenAiResponsesV1)?;
    if request.continuation.is_some() {
        return Err(ShapeAdapterError::new(
            "unsupported_responses_capability",
            "responses v1 requires item continuations that are not resolved",
        ));
    }
    let mut input = Vec::new();
    for instruction in &request.instructions {
        let role = match instruction.role {
            InstructionRole::Policy | InstructionRole::System => "system",
            InstructionRole::Developer => "developer",
            InstructionRole::Context => "user",
        };
        input.push(json!({
            "type":"message",
            "role":role,
            "content":openai_responses_content(&instruction.content, resources)?,
        }));
    }
    let mut call_ids = HashMap::new();
    for item in &request.transcript {
        match item {
            LlmTurnItemIr::Message { role, content, .. } => input.push(json!({
                "type":"message",
                "role":match role { MessageRole::User => "user", MessageRole::Assistant => "assistant" },
                "content":openai_responses_content(content, resources)?,
            })),
            LlmTurnItemIr::AssistantToolCall { id, call } => {
                let provider_id = call.provider_call_id.as_deref().unwrap_or(&call.id);
                call_ids.insert(call.id.as_str(), provider_id.to_owned());
                input.push(json!({
                    "type":"function_call",
                    "id":id,
                    "call_id":provider_id,
                    "name":call.name,
                    "arguments":canonical::to_string(&call.arguments).map_err(|_| {
                        ShapeAdapterError::new("invalid_tool_arguments", "tool arguments cannot be serialized")
                    })?,
                    "status":"completed",
                }));
            }
            LlmTurnItemIr::ToolResult {
                id,
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
                input.push(json!({
                    "type":"function_call_output",
                    "id":id,
                    "call_id":provider_id,
                    "output":text_only(content)?,
                    "status":"completed",
                }));
            }
            LlmTurnItemIr::HostedTool {
                opaque_item_ref: Some(reference),
                ..
            }
            | LlmTurnItemIr::Reasoning {
                opaque_item_ref: Some(reference),
                ..
            } => input.push(opaque_value(reference, resources)?),
            LlmTurnItemIr::HostedTool { .. } | LlmTurnItemIr::Reasoning { .. } => {
                return Err(ShapeAdapterError::new(
                    "opaque_roundtrip_material_missing",
                    "responses hosted/reasoning item has no same-shape opaque material",
                ));
            }
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("input".into(), Value::Array(input));
    body.insert("stream".into(), Value::Bool(options.stream));
    body.insert(
        "max_output_tokens".into(),
        Value::Number(u32_limit(options.max_output_tokens)?.into()),
    );
    apply_generation(&mut body, request, options.max_output_tokens)?;
    apply_tools(&mut body, request)?;
    apply_response_format(&mut body, request);
    if let Some(extension) = request
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.openai.as_ref())
    {
        apply_extension_fields(
            &mut body,
            extension,
            &["background", "parallel_tool_calls", "store", "truncation"],
        )?;
    }
    finish_request::<gproxy_protocol::openai::ResponseCreateRequest>(
        ShapeAdapterKey::OpenAiResponsesV1,
        pin,
        target,
        Value::Object(body),
    )
}

pub fn decode_openai_responses_terminal(
    pin: &LlmOperationExecutionPin,
    model_call_id: &str,
    bytes: &[u8],
) -> Result<DecodedTerminalDraft, ShapeAdapterError> {
    let descriptor = super::resolve_shape_adapter(pin)?;
    if descriptor.key != ShapeAdapterKey::OpenAiResponsesV1 {
        return Err(ShapeAdapterError::new(
            "adapter_execution_mismatch",
            "responses terminal decoder does not match execution pin",
        ));
    }
    let value = parse_typed_terminal::<gproxy_protocol::openai::ResponseObject>(bytes)?;
    reject_nonterminal_or_failed(&value)?;
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ShapeAdapterError::new("responses_output_missing", "responses output is missing")
        })?;
    let mut items = Vec::new();
    let mut sensitive_entries = Vec::new();
    let mut opaque_attachments = Vec::new();
    let mut has_tool_calls = false;
    for (index, item) in output.iter().enumerate() {
        let kind = required_string(item, "type", "responses_item_type_missing")?;
        match kind {
            "message" => decode_message(model_call_id, index, item, &mut items)?,
            "function_call" => {
                has_tool_calls = true;
                decode_function_call(model_call_id, index, item, &mut items)?;
            }
            "reasoning" => push_opaque_reasoning(
                model_call_id,
                index,
                item,
                &mut items,
                &mut sensitive_entries,
                &mut opaque_attachments,
            )?,
            other => push_opaque_hosted(
                model_call_id,
                index,
                other,
                item,
                &mut items,
                &mut sensitive_entries,
                &mut opaque_attachments,
            )?,
        }
    }
    let response = LlmResponseIr {
        model_call_id: model_call_id.into(),
        items,
        usage: decode_usage(value.get("usage")),
        finish_reason: Some(decode_finish_reason(&value, has_tool_calls)),
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
    if generation.seed.is_some() || !generation.stop.is_empty() {
        return Err(ShapeAdapterError::new(
            "unsupported_generation_option",
            "responses v1 does not support seed or stop in this adapter version",
        ));
    }
    if let Some(value) = generation.temperature {
        body.insert("temperature".into(), json!(value));
    }
    if let Some(value) = generation.top_p {
        body.insert("top_p".into(), json!(value));
    }
    if let Some(value) = generation.max_output_tokens {
        body.insert(
            "max_output_tokens".into(),
            json!(u32_limit(value.min(hard_output_limit))?),
        );
    }
    Ok(())
}

fn apply_tools(
    body: &mut Map<String, Value>,
    request: &LlmRequestIr,
) -> Result<(), ShapeAdapterError> {
    if !request.tools.is_empty() || !request.hosted_tools.is_empty() {
        let mut tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type":"function",
                    "name":tool.name,
                    "description":tool.description,
                    "parameters":tool.input_schema.document,
                    "strict":true,
                })
            })
            .collect();
        for tool in &request.hosted_tools {
            tools.push(super::openai_responses_hosted::encode_hosted_tool(tool)?);
        }
        body.insert("tools".into(), Value::Array(tools));
    }
    if let Some(choice) = &request.tool_choice {
        body.insert(
            "tool_choice".into(),
            match choice {
                ToolChoiceIr::Auto => json!("auto"),
                ToolChoiceIr::None => json!("none"),
                ToolChoiceIr::Required => json!("required"),
                ToolChoiceIr::Named { name } => json!({"type":"function","name":name}),
            },
        );
    }
    Ok(())
}

fn apply_response_format(body: &mut Map<String, Value>, request: &LlmRequestIr) {
    let Some(format) = &request.response_format else {
        return;
    };
    let format = match format {
        ResponseFormatIr::Text => json!({"type":"text"}),
        ResponseFormatIr::Json { schema: None, .. } => json!({"type":"json_object"}),
        ResponseFormatIr::Json {
            schema: Some(schema),
            strict,
        } => json!({
            "type":"json_schema",
            "name":"response",
            "schema":schema.document,
            "strict":strict,
        }),
    };
    body.insert("text".into(), json!({"format":format}));
}

fn decode_message(
    model_call_id: &str,
    index: usize,
    item: &Value,
    items: &mut Vec<LlmTurnItemIr>,
) -> Result<(), ShapeAdapterError> {
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ShapeAdapterError::new(
                "responses_message_content_missing",
                "response message has no content",
            )
        })?;
    let mut parts = Vec::new();
    for part in content {
        let kind = required_string(part, "type", "responses_content_type_missing")?;
        let field = match kind {
            "output_text" => "text",
            "refusal" => "refusal",
            _ => {
                return Err(ShapeAdapterError::new(
                    "unsupported_responses_content",
                    "response message contains an unsupported content part",
                ));
            }
        };
        let text = required_string(part, field, "responses_content_text_missing")?;
        if !text.is_empty() {
            parts.push(LlmContentPartIr::Text { text: text.into() });
        }
    }
    if !parts.is_empty() {
        items.push(LlmTurnItemIr::Message {
            id: format!("{model_call_id}:message:{index}"),
            role: MessageRole::Assistant,
            content: parts,
            provenance: None,
            placeholder: false,
        });
    }
    Ok(())
}

fn decode_function_call(
    model_call_id: &str,
    index: usize,
    item: &Value,
    items: &mut Vec<LlmTurnItemIr>,
) -> Result<(), ShapeAdapterError> {
    let provider_id = required_string(item, "call_id", "responses_call_id_missing")?;
    let name = required_string(item, "name", "responses_tool_name_missing")?;
    let raw_arguments = required_string(item, "arguments", "responses_tool_arguments_missing")?;
    let arguments = serde_json::from_str(raw_arguments).map_err(|_| {
        ShapeAdapterError::new(
            "responses_tool_arguments_invalid",
            "responses tool arguments are not complete JSON",
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
    Ok(())
}

fn reject_nonterminal_or_failed(value: &Value) -> Result<(), ShapeAdapterError> {
    match value.get("status").and_then(Value::as_str) {
        Some("failed") => Err(ShapeAdapterError::new(
            "provider_terminal_failed",
            "responses provider returned a failed terminal",
        )),
        Some("in_progress" | "queued") => Err(ShapeAdapterError::new(
            "provider_terminal_incomplete",
            "responses object is not terminal",
        )),
        _ => Ok(()),
    }
}

fn decode_usage(value: Option<&Value>) -> Option<LlmUsageIr> {
    let value = value?;
    Some(LlmUsageIr {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        cached_input_tokens: value
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    })
}

fn decode_finish_reason(value: &Value, has_tool_calls: bool) -> LlmFinishReason {
    if has_tool_calls {
        return LlmFinishReason::ToolCalls;
    }
    match value.get("status").and_then(Value::as_str) {
        Some("completed") => LlmFinishReason::Completed,
        Some("cancelled") => LlmFinishReason::Cancelled,
        Some("incomplete") => match value
            .pointer("/incomplete_details/reason")
            .and_then(Value::as_str)
        {
            Some("max_output_tokens") => LlmFinishReason::Length,
            Some("content_filter") => LlmFinishReason::ContentFilter,
            _ => LlmFinishReason::Unknown,
        },
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
