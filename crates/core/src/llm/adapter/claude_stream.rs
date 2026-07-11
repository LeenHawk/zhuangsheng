use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::llm::{
    LlmOperationExecutionPin,
    ir::{LlmApiError, LlmStreamEventIr, LlmTurnItemIr, ToolCallIr},
};

use super::{
    DecodedStreamBatch, ShapeAdapterError, ShapeAdapterKey,
    common::{parse_typed_terminal, required_string, required_u64},
    decode_claude_terminal,
};

struct ClaudeBlockState {
    kind: String,
    block: Value,
    arguments: String,
    stopped: bool,
}

pub struct ClaudeStreamDecoder {
    pin: LlmOperationExecutionPin,
    model_call_id: String,
    next_seq: u64,
    started: bool,
    terminal: bool,
    provider_id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    stop_sequence: Option<Value>,
    usage: Map<String, Value>,
    blocks: BTreeMap<u64, ClaudeBlockState>,
}

impl ClaudeStreamDecoder {
    pub fn new(
        pin: LlmOperationExecutionPin,
        model_call_id: impl Into<String>,
    ) -> Result<Self, ShapeAdapterError> {
        if super::resolve_shape_adapter(&pin)?.key != ShapeAdapterKey::ClaudeMessagesV1 {
            return Err(ShapeAdapterError::new(
                "adapter_execution_mismatch",
                "Claude stream decoder does not match execution pin",
            ));
        }
        let model_call_id = model_call_id.into();
        if model_call_id.is_empty() || model_call_id.len() > 96 {
            return Err(ShapeAdapterError::new(
                "invalid_model_call_id",
                "stream model call id is empty or too long",
            ));
        }
        Ok(Self {
            pin,
            model_call_id,
            next_seq: 0,
            started: false,
            terminal: false,
            provider_id: None,
            model: None,
            stop_reason: None,
            stop_sequence: None,
            usage: Map::new(),
            blocks: BTreeMap::new(),
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        if self.terminal {
            return Err(ShapeAdapterError::new(
                "stream_after_terminal",
                "Claude stream event arrived after terminal",
            ));
        }
        let value = parse_typed_terminal::<gproxy_protocol::claude::StreamEvent>(bytes)?;
        let kind = value.get("type").and_then(Value::as_str).ok_or_else(|| {
            ShapeAdapterError::new(
                "claude_stream_type_missing",
                "Claude stream event type is missing",
            )
        })?;
        let mut batch = DecodedStreamBatch::default();
        match kind {
            "message_start" => self.message_start(&value, &mut batch)?,
            "content_block_start" => self.block_start(&value, &mut batch)?,
            "content_block_delta" => self.block_delta(&value, &mut batch)?,
            "content_block_stop" => self.block_stop(&value, &mut batch)?,
            "message_delta" => self.message_delta(&value)?,
            "message_stop" => self.message_stop(&mut batch)?,
            "error" => self.stream_error(&value, &mut batch),
            "ping" => self.require_started()?,
            _ => {
                return Err(ShapeAdapterError::new(
                    "unsupported_claude_stream_event",
                    "Claude stream event is unknown to this decoder version",
                ));
            }
        }
        Ok(batch)
    }

    pub fn finish(&self) -> Result<(), ShapeAdapterError> {
        if self.terminal {
            Ok(())
        } else {
            Err(ShapeAdapterError::new(
                "stream_terminal_missing",
                "Claude stream ended without message_stop or error",
            ))
        }
    }

    fn message_start(
        &mut self,
        value: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        if self.started {
            return Err(ShapeAdapterError::new(
                "duplicate_stream_start",
                "Claude stream started more than once",
            ));
        }
        let message = value.get("message").ok_or_else(|| {
            ShapeAdapterError::new(
                "claude_message_start_missing",
                "Claude start message is missing",
            )
        })?;
        let content = message
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ShapeAdapterError::new(
                    "claude_start_content_missing",
                    "Claude start content is missing",
                )
            })?;
        if !content.is_empty() {
            return Err(ShapeAdapterError::new(
                "claude_start_content_not_empty",
                "Claude stream start must not preload content blocks",
            ));
        }
        self.provider_id =
            Some(required_string(message, "id", "claude_message_id_missing")?.into());
        self.model = Some(required_string(message, "model", "claude_model_missing")?.into());
        merge_object(&mut self.usage, message.get("usage"));
        self.started = true;
        batch.events.push(LlmStreamEventIr::Started {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
        });
        Ok(())
    }

    fn block_start(
        &mut self,
        value: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        self.require_started()?;
        let index = required_u64(value, "index", "claude_block_index_missing")?;
        if self.blocks.contains_key(&index) {
            return Err(ShapeAdapterError::new(
                "duplicate_claude_block",
                "Claude content block started more than once",
            ));
        }
        if index != self.blocks.len() as u64 {
            return Err(ShapeAdapterError::new(
                "claude_block_index_gap",
                "Claude content block indexes must be contiguous from zero",
            ));
        }
        let block = value.get("content_block").cloned().ok_or_else(|| {
            ShapeAdapterError::new(
                "claude_content_block_missing",
                "Claude content block is missing",
            )
        })?;
        let kind = required_string(&block, "type", "claude_block_type_missing")?.to_owned();
        if kind == "tool_use" {
            let name = required_string(&block, "name", "claude_tool_name_missing")?;
            batch.events.push(LlmStreamEventIr::ToolCallDelta {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                item_id: self.item_id("tool", index),
                tool_call_id: self.item_id("call", index),
                name: Some(name.into()),
                arguments_delta: None,
            });
        }
        self.blocks.insert(
            index,
            ClaudeBlockState {
                kind,
                block,
                arguments: String::new(),
                stopped: false,
            },
        );
        Ok(())
    }

    fn block_delta(
        &mut self,
        value: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        self.require_started()?;
        let index = required_u64(value, "index", "claude_block_index_missing")?;
        let delta = value.get("delta").ok_or_else(|| {
            ShapeAdapterError::new("claude_delta_missing", "Claude block delta is missing")
        })?;
        let delta_kind = required_string(delta, "type", "claude_delta_type_missing")?;
        let state = self.blocks.get_mut(&index).ok_or_else(|| {
            ShapeAdapterError::new(
                "claude_block_start_missing",
                "Claude delta arrived before content block start",
            )
        })?;
        if state.stopped {
            return Err(ShapeAdapterError::new(
                "claude_delta_after_block_stop",
                "Claude delta arrived after content block stop",
            ));
        }
        match delta_kind {
            "text_delta" if state.kind == "text" => {
                let text = required_string(delta, "text", "claude_text_delta_missing")?;
                append_string_field(&mut state.block, "text", text)?;
                if !text.is_empty() {
                    batch.events.push(LlmStreamEventIr::TextDelta {
                        call_id: self.model_call_id.clone(),
                        seq: self.take_seq(),
                        item_id: self.item_id("message", index),
                        text: text.into(),
                    });
                }
            }
            "thinking_delta" if state.kind == "thinking" => {
                let text = required_string(delta, "thinking", "claude_thinking_delta_missing")?;
                append_string_field(&mut state.block, "thinking", text)?;
                if !text.is_empty() {
                    batch.events.push(LlmStreamEventIr::ReasoningDelta {
                        call_id: self.model_call_id.clone(),
                        seq: self.take_seq(),
                        item_id: self.item_id("reasoning", index),
                        text: text.into(),
                    });
                }
            }
            "signature_delta" if state.kind == "thinking" => {
                let signature =
                    required_string(delta, "signature", "claude_signature_delta_missing")?;
                append_string_field(&mut state.block, "signature", signature)?;
            }
            "input_json_delta" if state.kind == "tool_use" => {
                let partial = required_string(delta, "partial_json", "claude_input_delta_missing")?;
                state.arguments.push_str(partial);
                if !partial.is_empty() {
                    batch.events.push(LlmStreamEventIr::ToolCallDelta {
                        call_id: self.model_call_id.clone(),
                        seq: self.take_seq(),
                        item_id: self.item_id("tool", index),
                        tool_call_id: self.item_id("call", index),
                        name: None,
                        arguments_delta: Some(partial.into()),
                    });
                }
            }
            "citations_delta" => {}
            _ => {
                return Err(ShapeAdapterError::new(
                    "claude_delta_type_mismatch",
                    "Claude delta type does not match its content block",
                ));
            }
        }
        Ok(())
    }

    fn block_stop(
        &mut self,
        value: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        self.require_started()?;
        let index = required_u64(value, "index", "claude_block_index_missing")?;
        let completed = {
            let state = self.blocks.get_mut(&index).ok_or_else(|| {
                ShapeAdapterError::new(
                    "claude_block_start_missing",
                    "Claude block stopped before it started",
                )
            })?;
            if state.stopped {
                return Err(ShapeAdapterError::new(
                    "duplicate_claude_block_stop",
                    "Claude content block stopped more than once",
                ));
            }
            state.stopped = true;
            if state.kind == "tool_use" {
                let arguments = if state.arguments.is_empty() {
                    state
                        .block
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| json!({}))
                } else {
                    serde_json::from_str(&state.arguments).map_err(|_| {
                        ShapeAdapterError::new(
                            "claude_tool_arguments_invalid",
                            "Claude tool argument deltas are not complete JSON",
                        )
                    })?
                };
                if !arguments.is_object() {
                    return Err(ShapeAdapterError::new(
                        "claude_tool_arguments_not_object",
                        "Claude tool arguments must be a JSON object",
                    ));
                }
                state.block["input"] = arguments.clone();
                let provider_id =
                    required_string(&state.block, "id", "claude_tool_id_missing")?.to_owned();
                let name =
                    required_string(&state.block, "name", "claude_tool_name_missing")?.to_owned();
                Some((provider_id, name, arguments))
            } else {
                None
            }
        };
        if let Some((provider_id, name, arguments)) = completed {
            batch.events.push(LlmStreamEventIr::ToolCallCompleted {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                item: LlmTurnItemIr::AssistantToolCall {
                    id: self.item_id("tool", index),
                    call: ToolCallIr {
                        id: self.item_id("call", index),
                        provider_call_id: Some(provider_id),
                        name,
                        arguments,
                    },
                },
            });
        }
        Ok(())
    }

    fn message_delta(&mut self, value: &Value) -> Result<(), ShapeAdapterError> {
        self.require_started()?;
        let delta = value.get("delta").ok_or_else(|| {
            ShapeAdapterError::new(
                "claude_message_delta_missing",
                "Claude message delta is missing",
            )
        })?;
        if let Some(reason) = delta.get("stop_reason").and_then(Value::as_str) {
            self.stop_reason = Some(reason.into());
        }
        if let Some(sequence) = delta.get("stop_sequence") {
            self.stop_sequence = Some(sequence.clone());
        }
        merge_object(&mut self.usage, value.get("usage"));
        Ok(())
    }

    fn message_stop(&mut self, batch: &mut DecodedStreamBatch) -> Result<(), ShapeAdapterError> {
        self.require_started()?;
        if self.blocks.values().any(|state| !state.stopped) {
            return Err(ShapeAdapterError::new(
                "claude_block_terminal_missing",
                "Claude message stopped with an open content block",
            ));
        }
        let content: Vec<_> = self
            .blocks
            .values()
            .map(|state| state.block.clone())
            .collect();
        let terminal = json!({
            "id":self.provider_id.as_deref().unwrap_or("claude-stream"),
            "type":"message",
            "role":"assistant",
            "content":content,
            "model":self.model.as_deref().unwrap_or(&self.pin.model_id),
            "stop_reason":self.stop_reason.as_deref().ok_or_else(|| {
                ShapeAdapterError::new("claude_stop_reason_missing", "Claude stop reason is missing")
            })?,
            "stop_sequence":self.stop_sequence.clone().unwrap_or(Value::Null),
            "usage":self.usage,
        });
        let decoded = decode_claude_terminal(
            &self.pin,
            &self.model_call_id,
            &serde_json::to_vec(&terminal).map_err(|_| {
                ShapeAdapterError::new(
                    "wire_terminal_decode_failed",
                    "Claude stream terminal cannot be serialized",
                )
            })?,
        )?;
        if let Some(usage) = decoded.response.usage.clone() {
            batch.events.push(LlmStreamEventIr::Usage {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                usage,
            });
        }
        batch.events.push(LlmStreamEventIr::Completed {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
            response: decoded.response,
        });
        batch.sensitive_entries = decoded.sensitive_entries;
        batch.opaque_attachments = decoded.opaque_attachments;
        self.terminal = true;
        Ok(())
    }

    fn stream_error(&mut self, value: &Value, batch: &mut DecodedStreamBatch) {
        let code = value
            .pointer("/error/type")
            .and_then(Value::as_str)
            .map(|value| value.chars().take(128).collect());
        batch.events.push(LlmStreamEventIr::Failed {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
            error: LlmApiError {
                operation_key: self.pin.operation_key,
                operation_taxonomy_version: self.pin.operation_taxonomy_version,
                adapter_decoder_version: self.pin.adapter_decoder_version,
                status_code: None,
                code,
                message: "Claude message stream failed".into(),
                retryable: false,
            },
        });
        self.terminal = true;
    }

    fn require_started(&self) -> Result<(), ShapeAdapterError> {
        if self.started {
            Ok(())
        } else {
            Err(ShapeAdapterError::new(
                "stream_start_missing",
                "Claude stream event arrived before message_start",
            ))
        }
    }

    fn item_id(&self, kind: &str, index: u64) -> String {
        format!("{}:{kind}:{index}", self.model_call_id)
    }

    fn take_seq(&mut self) -> u64 {
        let value = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        value
    }
}

fn append_string_field(
    value: &mut Value,
    field: &str,
    delta: &str,
) -> Result<(), ShapeAdapterError> {
    let existing = value.get(field).and_then(Value::as_str).ok_or_else(|| {
        ShapeAdapterError::new(
            "claude_block_state_invalid",
            "Claude block string field is missing",
        )
    })?;
    let mut combined = existing.to_owned();
    combined.push_str(delta);
    value[field] = Value::String(combined);
    Ok(())
}

fn merge_object(target: &mut Map<String, Value>, value: Option<&Value>) {
    if let Some(source) = value.and_then(Value::as_object) {
        for (key, value) in source {
            if !value.is_null() {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}
