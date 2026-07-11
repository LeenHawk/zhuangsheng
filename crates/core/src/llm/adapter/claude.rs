use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::{
    canonical,
    graph::ToolChoiceIr,
    llm::{
        LlmOperationExecutionPin,
        ir::{
            HostedToolPhase, InstructionRole, LlmContentPartIr, LlmFinishReason, LlmRequestIr,
            LlmResponseIr, LlmTurnItemIr, LlmUsageIr, MessageRole, ResponseFormatIr, ToolCallIr,
            ToolResultOutcome,
        },
    },
};

use super::{
    AdapterExecutionOptions, AdapterResources, DecodedTerminalDraft, OpaqueAttachmentDraft,
    OpaqueAttachmentTarget, SensitiveEntryDraft, ShapeAdapterError, ShapeAdapterKey,
    WireGenerationRequest,
    common::{
        apply_extension_fields, finish_request, material_base64, opaque_value,
        parse_typed_terminal, prepare, required_string, text_only,
    },
};

pub fn encode_claude_request(
    pin: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    resources: &AdapterResources,
    options: AdapterExecutionOptions,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    let target = prepare(pin, request, options, ShapeAdapterKey::ClaudeMessagesV1)?;
    if request.continuation.is_some() || !request.hosted_tools.is_empty() {
        return Err(ShapeAdapterError::new(
            "unsupported_claude_capability",
            "claude v1 has no generic top-level continuation or hosted-tool mapping",
        ));
    }
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for instruction in &request.instructions {
        match instruction.role {
            InstructionRole::Policy | InstructionRole::System | InstructionRole::Developer => {
                for part in &instruction.content {
                    let LlmContentPartIr::Text { text } = part else {
                        return Err(ShapeAdapterError::new(
                            "unsupported_system_content",
                            "claude system instructions only accept text",
                        ));
                    };
                    system.push(json!({"type":"text","text":text}));
                }
            }
            InstructionRole::Context => {
                for block in claude_content(&instruction.content, resources)? {
                    push_message_block(&mut messages, "user", block);
                }
            }
        }
    }
    let mut call_ids = HashMap::new();
    for item in &request.transcript {
        match item {
            LlmTurnItemIr::Message { role, content, .. } => {
                let role = match role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                };
                for block in claude_content(content, resources)? {
                    push_message_block(&mut messages, role, block);
                }
            }
            LlmTurnItemIr::AssistantToolCall { call, .. } => {
                let provider_id = call.provider_call_id.as_deref().unwrap_or(&call.id);
                let arguments = call.arguments.as_object().ok_or_else(|| {
                    ShapeAdapterError::new(
                        "claude_tool_arguments_not_object",
                        "claude tool input must be a JSON object",
                    )
                })?;
                call_ids.insert(call.id.as_str(), provider_id.to_owned());
                push_message_block(
                    &mut messages,
                    "assistant",
                    json!({
                        "type":"tool_use",
                        "id":provider_id,
                        "name":call.name,
                        "input":arguments,
                    }),
                );
            }
            LlmTurnItemIr::ToolResult {
                tool_call_id,
                outcome,
                content,
                ..
            } => {
                let provider_id = call_ids.get(tool_call_id.as_str()).ok_or_else(|| {
                    ShapeAdapterError::new(
                        "tool_result_call_missing",
                        "tool result cannot recover provider tool-use id",
                    )
                })?;
                push_message_block(
                    &mut messages,
                    "user",
                    json!({
                        "type":"tool_result",
                        "tool_use_id":provider_id,
                        "content":text_only(content)?,
                        "is_error":!matches!(outcome, ToolResultOutcome::Success),
                    }),
                );
            }
            LlmTurnItemIr::HostedTool {
                opaque_item_ref: Some(reference),
                ..
            }
            | LlmTurnItemIr::Reasoning {
                opaque_item_ref: Some(reference),
                ..
            } => push_message_block(
                &mut messages,
                "assistant",
                opaque_value(reference, resources)?,
            ),
            LlmTurnItemIr::HostedTool { .. } | LlmTurnItemIr::Reasoning { .. } => {
                return Err(ShapeAdapterError::new(
                    "opaque_roundtrip_material_missing",
                    "claude hosted/reasoning item has no same-shape opaque material",
                ));
            }
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("messages".into(), Value::Array(messages));
    body.insert(
        "max_tokens".into(),
        Value::Number(options.max_output_tokens.into()),
    );
    body.insert("stream".into(), Value::Bool(options.stream));
    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }
    apply_generation(&mut body, request, options.max_output_tokens)?;
    apply_tools(&mut body, request);
    apply_response_format(&mut body, request)?;
    if let Some(extension) = request
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.claude.as_ref())
    {
        apply_extension_fields(
            &mut body,
            extension,
            &["inference_geo", "service_tier", "speed"],
        )?;
    }
    finish_request::<gproxy_protocol::claude::CreateMessageRequestBody>(
        ShapeAdapterKey::ClaudeMessagesV1,
        pin,
        target,
        Value::Object(body),
    )
}

pub fn decode_claude_terminal(
    pin: &LlmOperationExecutionPin,
    model_call_id: &str,
    bytes: &[u8],
) -> Result<DecodedTerminalDraft, ShapeAdapterError> {
    let descriptor = super::resolve_shape_adapter(pin)?;
    if descriptor.key != ShapeAdapterKey::ClaudeMessagesV1 {
        return Err(ShapeAdapterError::new(
            "adapter_execution_mismatch",
            "claude terminal decoder does not match execution pin",
        ));
    }
    let value = parse_typed_terminal::<gproxy_protocol::claude::CreateMessageResponseBody>(bytes)?;
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ShapeAdapterError::new("claude_content_missing", "claude content is missing")
        })?;
    let mut items = Vec::new();
    let mut sensitive_entries = Vec::new();
    let mut opaque_attachments = Vec::new();
    let mut has_tool_calls = false;
    for (index, block) in content.iter().enumerate() {
        let kind = required_string(block, "type", "claude_block_type_missing")?;
        match kind {
            "text" => {
                let text = required_string(block, "text", "claude_text_missing")?;
                if !text.is_empty() {
                    items.push(LlmTurnItemIr::Message {
                        id: format!("{model_call_id}:message:{index}"),
                        role: MessageRole::Assistant,
                        content: vec![LlmContentPartIr::Text { text: text.into() }],
                        provenance: None,
                    });
                }
            }
            "tool_use" => {
                has_tool_calls = true;
                let provider_id = required_string(block, "id", "claude_tool_id_missing")?;
                let name = required_string(block, "name", "claude_tool_name_missing")?;
                let arguments = block.get("input").cloned().ok_or_else(|| {
                    ShapeAdapterError::new(
                        "claude_tool_input_missing",
                        "claude tool input is missing",
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
            "thinking" | "redacted_thinking" => push_reasoning_sidecar(
                model_call_id,
                index,
                block,
                &mut items,
                &mut sensitive_entries,
                &mut opaque_attachments,
            )?,
            other => push_hosted_sidecar(
                model_call_id,
                index,
                other,
                block,
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
        finish_reason: Some(decode_finish_reason(
            value.get("stop_reason").and_then(Value::as_str),
            has_tool_calls,
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

fn claude_content(
    parts: &[LlmContentPartIr],
    resources: &AdapterResources,
) -> Result<Vec<Value>, ShapeAdapterError> {
    parts
        .iter()
        .map(|part| match part {
            LlmContentPartIr::Text { text } => Ok(json!({"type":"text","text":text})),
            LlmContentPartIr::Image { artifact_ref } => Ok(json!({
                "type":"image",
                "source":{
                    "type":"base64",
                    "media_type":artifact_ref.media_type,
                    "data":material_base64(artifact_ref, resources)?,
                }
            })),
            LlmContentPartIr::File { artifact_ref }
                if artifact_ref.media_type == "application/pdf" =>
            {
                Ok(json!({
                    "type":"document",
                    "source":{
                        "type":"base64",
                        "media_type":"application/pdf",
                        "data":material_base64(artifact_ref, resources)?,
                    }
                }))
            }
            LlmContentPartIr::File { .. } => Err(ShapeAdapterError::new(
                "unsupported_claude_file_type",
                "claude v1 only accepts PDF file parts",
            )),
        })
        .collect()
}

fn push_message_block(messages: &mut Vec<Value>, role: &str, block: Value) {
    if let Some(content) = messages.last_mut().and_then(|message| {
        (message.get("role").and_then(Value::as_str) == Some(role))
            .then(|| message.get_mut("content").and_then(Value::as_array_mut))
            .flatten()
    }) {
        content.push(block);
    } else {
        messages.push(json!({"role":role,"content":[block]}));
    }
}

fn apply_generation(
    body: &mut Map<String, Value>,
    request: &LlmRequestIr,
    hard_limit: u64,
) -> Result<(), ShapeAdapterError> {
    let Some(generation) = &request.generation else {
        return Ok(());
    };
    if generation.seed.is_some() {
        return Err(ShapeAdapterError::new(
            "unsupported_generation_option",
            "claude v1 does not support seed in this adapter version",
        ));
    }
    if let Some(value) = generation.temperature {
        body.insert("temperature".into(), json!(value));
    }
    if let Some(value) = generation.top_p {
        body.insert("top_p".into(), json!(value));
    }
    if !generation.stop.is_empty() {
        body.insert("stop_sequences".into(), json!(generation.stop));
    }
    if let Some(value) = generation.max_output_tokens {
        body.insert("max_tokens".into(), json!(value.min(hard_limit)));
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
                            "name":tool.name,
                            "description":tool.description,
                            "input_schema":tool.input_schema.document,
                            "strict":true,
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
                ToolChoiceIr::Auto => json!({"type":"auto"}),
                ToolChoiceIr::None => json!({"type":"none"}),
                ToolChoiceIr::Required => json!({"type":"any"}),
                ToolChoiceIr::Named { name } => json!({"type":"tool","name":name}),
            },
        );
    }
}

fn apply_response_format(
    body: &mut Map<String, Value>,
    request: &LlmRequestIr,
) -> Result<(), ShapeAdapterError> {
    match &request.response_format {
        None | Some(ResponseFormatIr::Text) => Ok(()),
        Some(ResponseFormatIr::Json {
            schema: Some(schema),
            ..
        }) => {
            body.insert(
                "output_config".into(),
                json!({"format":{"type":"json_schema","schema":schema.document}}),
            );
            Ok(())
        }
        Some(ResponseFormatIr::Json { schema: None, .. }) => Err(ShapeAdapterError::new(
            "claude_json_schema_required",
            "claude structured output requires an explicit JSON schema",
        )),
    }
}

fn push_reasoning_sidecar(
    model_call_id: &str,
    index: usize,
    block: &Value,
    items: &mut Vec<LlmTurnItemIr>,
    entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let id = format!("{model_call_id}:reasoning:{index}");
    items.push(LlmTurnItemIr::Reasoning {
        id: id.clone(),
        summary: None,
        opaque_item_ref: None,
    });
    push_sidecar(index, "thinking", block, id, entries, attachments)
}

fn push_hosted_sidecar(
    model_call_id: &str,
    index: usize,
    kind: &str,
    block: &Value,
    items: &mut Vec<LlmTurnItemIr>,
    entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let id = format!("{model_call_id}:hosted:{index}");
    items.push(LlmTurnItemIr::HostedTool {
        id: id.clone(),
        binding_id: kind.into(),
        kind: kind.into(),
        phase: HostedToolPhase::Completed,
        display_content: Vec::new(),
        opaque_item_ref: None,
    });
    push_sidecar(index, kind, block, id, entries, attachments)
}

fn push_sidecar(
    index: usize,
    slot: &str,
    block: &Value,
    item_id: String,
    entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let entry_key = format!("claude_block_{index}");
    entries.push(SensitiveEntryDraft {
        entry_key: entry_key.clone(),
        adapter_key: ShapeAdapterKey::ClaudeMessagesV1,
        semantic_slot: slot.into(),
        opaque_bytes: canonical::to_vec(block).map_err(|_| {
            ShapeAdapterError::new(
                "opaque_item_encode_failed",
                "claude sidecar could not be encoded",
            )
        })?,
    });
    attachments.push(OpaqueAttachmentDraft {
        entry_key,
        target: OpaqueAttachmentTarget::Item { item_id },
    });
    Ok(())
}

fn decode_usage(value: Option<&Value>) -> Option<LlmUsageIr> {
    let value = value?;
    let input = value.get("input_tokens").and_then(Value::as_u64);
    let output = value.get("output_tokens").and_then(Value::as_u64);
    Some(LlmUsageIr {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input.zip(output).and_then(|(a, b)| a.checked_add(b)),
        cached_input_tokens: value.get("cache_read_input_tokens").and_then(Value::as_u64),
        reasoning_tokens: value
            .pointer("/output_tokens_details/thinking_tokens")
            .and_then(Value::as_u64),
    })
}

fn decode_finish_reason(value: Option<&str>, has_tool_calls: bool) -> LlmFinishReason {
    if has_tool_calls {
        return LlmFinishReason::ToolCalls;
    }
    match value {
        Some("end_turn" | "stop_sequence") => LlmFinishReason::Completed,
        Some("max_tokens" | "model_context_window_exceeded") => LlmFinishReason::Length,
        Some("refusal") => LlmFinishReason::ContentFilter,
        _ => LlmFinishReason::Unknown,
    }
}
