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

pub fn encode_gemini_request(
    pin: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    resources: &AdapterResources,
    options: AdapterExecutionOptions,
) -> Result<WireGenerationRequest, ShapeAdapterError> {
    let target = prepare(
        pin,
        request,
        options,
        ShapeAdapterKey::GeminiGenerateContentV1,
    )?;
    if request.continuation.is_some() || !request.hosted_tools.is_empty() {
        return Err(ShapeAdapterError::new(
            "unsupported_gemini_capability",
            "gemini v1 has no generic top-level continuation or hosted-tool mapping",
        ));
    }
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();
    for instruction in &request.instructions {
        match instruction.role {
            InstructionRole::Policy | InstructionRole::System | InstructionRole::Developer => {
                for part in &instruction.content {
                    let LlmContentPartIr::Text { text } = part else {
                        return Err(ShapeAdapterError::new(
                            "unsupported_system_content",
                            "gemini system instructions only accept text",
                        ));
                    };
                    system_parts.push(json!({"text":text}));
                }
            }
            InstructionRole::Context => {
                for part in gemini_parts(&instruction.content, resources)? {
                    push_content_part(&mut contents, "user", part);
                }
            }
        }
    }
    let mut calls = HashMap::new();
    for item in &request.transcript {
        match item {
            LlmTurnItemIr::Message { role, content, .. } => {
                let role = match role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "model",
                };
                for part in gemini_parts(content, resources)? {
                    push_content_part(&mut contents, role, part);
                }
            }
            LlmTurnItemIr::AssistantToolCall { call, .. } => {
                let arguments = call.arguments.as_object().ok_or_else(|| {
                    ShapeAdapterError::new(
                        "gemini_tool_arguments_not_object",
                        "gemini function-call args must be a JSON object",
                    )
                })?;
                let provider_id = call.provider_call_id.as_deref().unwrap_or(&call.id);
                calls.insert(
                    call.id.as_str(),
                    (provider_id.to_owned(), call.name.clone()),
                );
                push_content_part(
                    &mut contents,
                    "model",
                    json!({"functionCall":{"id":provider_id,"name":call.name,"args":arguments}}),
                );
            }
            LlmTurnItemIr::ToolResult {
                tool_call_id,
                outcome,
                content,
                ..
            } => {
                let (provider_id, name) = calls.get(tool_call_id.as_str()).ok_or_else(|| {
                    ShapeAdapterError::new(
                        "tool_result_call_missing",
                        "tool result cannot recover Gemini function-call identity",
                    )
                })?;
                push_content_part(
                    &mut contents,
                    "function",
                    json!({
                        "functionResponse":{
                            "id":provider_id,
                            "name":name,
                            "response":{
                                "output":text_only(content)?,
                                "outcome":match outcome {
                                    ToolResultOutcome::Success => "success",
                                    ToolResultOutcome::Error => "error",
                                    ToolResultOutcome::Denied => "denied",
                                }
                            }
                        }
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
            } => push_content_part(&mut contents, "model", opaque_value(reference, resources)?),
            LlmTurnItemIr::HostedTool { .. } | LlmTurnItemIr::Reasoning { .. } => {
                return Err(ShapeAdapterError::new(
                    "opaque_roundtrip_material_missing",
                    "gemini hosted/reasoning item has no same-shape opaque material",
                ));
            }
        }
    }
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("contents".into(), Value::Array(contents));
    if !system_parts.is_empty() {
        body.insert(
            "systemInstruction".into(),
            json!({"role":"system","parts":system_parts}),
        );
    }
    body.insert(
        "generationConfig".into(),
        generation_config(request, options.max_output_tokens)?,
    );
    apply_tools(&mut body, request);
    if let Some(extension) = request
        .extensions
        .as_ref()
        .and_then(|extensions| extensions.gemini.as_ref())
    {
        apply_extension_fields(&mut body, extension, &["serviceTier", "store"])?;
    }
    finish_request::<gproxy_protocol::gemini::GenerateContentRequest>(
        ShapeAdapterKey::GeminiGenerateContentV1,
        pin,
        target,
        Value::Object(body),
    )
}

pub fn decode_gemini_terminal(
    pin: &LlmOperationExecutionPin,
    model_call_id: &str,
    bytes: &[u8],
) -> Result<DecodedTerminalDraft, ShapeAdapterError> {
    let descriptor = super::resolve_shape_adapter(pin)?;
    if descriptor.key != ShapeAdapterKey::GeminiGenerateContentV1 {
        return Err(ShapeAdapterError::new(
            "adapter_execution_mismatch",
            "gemini terminal decoder does not match execution pin",
        ));
    }
    let value = parse_typed_terminal::<gproxy_protocol::gemini::GenerateContentResponse>(bytes)?;
    let candidates = value
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ShapeAdapterError::new("gemini_candidates_missing", "Gemini candidates are missing")
        })?;
    if candidates.len() != 1 {
        return Err(ShapeAdapterError::new(
            "gemini_candidate_cardinality",
            "gemini adapter requires exactly one candidate",
        ));
    }
    let candidate = &candidates[0];
    let parts = candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ShapeAdapterError::new(
                "gemini_content_missing",
                "Gemini candidate content is missing",
            )
        })?;
    let mut items = Vec::new();
    let mut sensitive_entries = Vec::new();
    let mut opaque_attachments = Vec::new();
    let mut has_tool_calls = false;
    for (index, part) in parts.iter().enumerate() {
        if part.get("thought").and_then(Value::as_bool) == Some(true)
            || part.get("thoughtSignature").is_some()
        {
            push_reasoning_sidecar(
                model_call_id,
                index,
                part,
                &mut items,
                &mut sensitive_entries,
                &mut opaque_attachments,
            )?;
        } else if let Some(text) = part.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                items.push(LlmTurnItemIr::Message {
                    id: format!("{model_call_id}:message:{index}"),
                    role: MessageRole::Assistant,
                    content: vec![LlmContentPartIr::Text { text: text.into() }],
                    provenance: None,
                });
            }
        } else if let Some(call) = part.get("functionCall") {
            has_tool_calls = true;
            let name = required_string(call, "name", "gemini_tool_name_missing")?;
            let arguments = call.get("args").cloned().unwrap_or_else(|| json!({}));
            items.push(LlmTurnItemIr::AssistantToolCall {
                id: format!("{model_call_id}:tool:{index}"),
                call: ToolCallIr {
                    id: format!("{model_call_id}:call:{index}"),
                    provider_call_id: call.get("id").and_then(Value::as_str).map(str::to_owned),
                    name: name.into(),
                    arguments,
                },
            });
        } else {
            push_hosted_sidecar(
                model_call_id,
                index,
                part,
                &mut items,
                &mut sensitive_entries,
                &mut opaque_attachments,
            )?;
        }
    }
    let response = LlmResponseIr {
        model_call_id: model_call_id.into(),
        items,
        usage: decode_usage(value.get("usageMetadata")),
        finish_reason: Some(decode_finish_reason(
            candidate.get("finishReason").and_then(Value::as_str),
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

fn gemini_parts(
    parts: &[LlmContentPartIr],
    resources: &AdapterResources,
) -> Result<Vec<Value>, ShapeAdapterError> {
    parts
        .iter()
        .map(|part| match part {
            LlmContentPartIr::Text { text } => Ok(json!({"text":text})),
            LlmContentPartIr::Image { artifact_ref } | LlmContentPartIr::File { artifact_ref } => {
                Ok(json!({
                    "inlineData":{
                        "mimeType":artifact_ref.media_type,
                        "data":material_base64(artifact_ref, resources)?,
                    }
                }))
            }
        })
        .collect()
}

fn push_content_part(contents: &mut Vec<Value>, role: &str, part: Value) {
    if let Some(parts) = contents.last_mut().and_then(|content| {
        (content.get("role").and_then(Value::as_str) == Some(role))
            .then(|| content.get_mut("parts").and_then(Value::as_array_mut))
            .flatten()
    }) {
        parts.push(part);
    } else {
        contents.push(json!({"role":role,"parts":[part]}));
    }
}

fn generation_config(request: &LlmRequestIr, hard_limit: u64) -> Result<Value, ShapeAdapterError> {
    let mut config = Map::new();
    config.insert("maxOutputTokens".into(), json!(i32_limit(hard_limit)?));
    if let Some(generation) = &request.generation {
        if let Some(value) = generation.temperature {
            config.insert("temperature".into(), json!(value));
        }
        if let Some(value) = generation.top_p {
            config.insert("topP".into(), json!(value));
        }
        if let Some(value) = generation.seed {
            config.insert("seed".into(), json!(value));
        }
        if !generation.stop.is_empty() {
            config.insert("stopSequences".into(), json!(generation.stop));
        }
        if let Some(value) = generation.max_output_tokens {
            config.insert(
                "maxOutputTokens".into(),
                json!(i32_limit(value.min(hard_limit))?),
            );
        }
    }
    match &request.response_format {
        None | Some(ResponseFormatIr::Text) => {
            config.insert("responseMimeType".into(), json!("text/plain"));
        }
        Some(ResponseFormatIr::Json { schema, .. }) => {
            config.insert("responseMimeType".into(), json!("application/json"));
            if let Some(schema) = schema {
                config.insert("responseJsonSchema".into(), schema.document.clone());
            }
        }
    }
    Ok(Value::Object(config))
}

fn apply_tools(body: &mut Map<String, Value>, request: &LlmRequestIr) {
    if !request.tools.is_empty() {
        body.insert(
            "tools".into(),
            json!([{"functionDeclarations":request.tools.iter().map(|tool| json!({
                "name":tool.name,
                "description":tool.description.clone().unwrap_or_default(),
                "parametersJsonSchema":tool.input_schema.document,
            })).collect::<Vec<_>>()}]),
        );
    }
    if let Some(choice) = &request.tool_choice {
        let (mode, names) = match choice {
            ToolChoiceIr::Auto => ("AUTO", Vec::new()),
            ToolChoiceIr::None => ("NONE", Vec::new()),
            ToolChoiceIr::Required => ("ANY", Vec::new()),
            ToolChoiceIr::Named { name } => ("ANY", vec![name.clone()]),
        };
        body.insert(
            "toolConfig".into(),
            json!({"functionCallingConfig":{"mode":mode,"allowedFunctionNames":names}}),
        );
    }
}

fn push_reasoning_sidecar(
    model_call_id: &str,
    index: usize,
    part: &Value,
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
    push_sidecar(index, "thought", part, id, entries, attachments)
}

fn push_hosted_sidecar(
    model_call_id: &str,
    index: usize,
    part: &Value,
    items: &mut Vec<LlmTurnItemIr>,
    entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let id = format!("{model_call_id}:hosted:{index}");
    items.push(LlmTurnItemIr::HostedTool {
        id: id.clone(),
        binding_id: "gemini_server_tool".into(),
        kind: "gemini_server_tool".into(),
        phase: HostedToolPhase::Completed,
        display_content: Vec::new(),
        opaque_item_ref: None,
    });
    push_sidecar(index, "hosted", part, id, entries, attachments)
}

fn push_sidecar(
    index: usize,
    slot: &str,
    part: &Value,
    item_id: String,
    entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let entry_key = format!("gemini_part_{index}");
    entries.push(SensitiveEntryDraft {
        entry_key: entry_key.clone(),
        adapter_key: ShapeAdapterKey::GeminiGenerateContentV1,
        semantic_slot: slot.into(),
        opaque_bytes: canonical::to_vec(part).map_err(|_| {
            ShapeAdapterError::new(
                "opaque_item_encode_failed",
                "Gemini sidecar could not be encoded",
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
    Some(LlmUsageIr {
        input_tokens: nonnegative(value.get("promptTokenCount")),
        output_tokens: nonnegative(value.get("candidatesTokenCount")),
        total_tokens: nonnegative(value.get("totalTokenCount")),
        cached_input_tokens: nonnegative(value.get("cachedContentTokenCount")),
        reasoning_tokens: nonnegative(value.get("thoughtsTokenCount")),
    })
}

fn nonnegative(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| u64::try_from(value).ok())
}

fn decode_finish_reason(value: Option<&str>, has_tool_calls: bool) -> LlmFinishReason {
    if has_tool_calls {
        return LlmFinishReason::ToolCalls;
    }
    match value {
        Some("STOP") => LlmFinishReason::Completed,
        Some("MAX_TOKENS") => LlmFinishReason::Length,
        Some("SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII") => {
            LlmFinishReason::ContentFilter
        }
        _ => LlmFinishReason::Unknown,
    }
}

fn i32_limit(value: u64) -> Result<i32, ShapeAdapterError> {
    i32::try_from(value).map_err(|_| {
        ShapeAdapterError::new(
            "wire_integer_limit",
            "token limit does not fit Gemini wire type",
        )
    })
}
