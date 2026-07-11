use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::llm::{
    LlmOperationExecutionPin,
    ir::{LlmStreamEventIr, LlmTurnItemIr, ToolCallIr},
};

use super::{
    DecodedStreamBatch, ShapeAdapterError, ShapeAdapterKey, common::parse_typed_terminal,
    decode_openai_chat_terminal,
};

#[derive(Default)]
struct ChatToolState {
    provider_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub struct OpenAiChatStreamDecoder {
    pin: LlmOperationExecutionPin,
    model_call_id: String,
    next_seq: u64,
    started: bool,
    finished: bool,
    provider_id: Option<String>,
    model: Option<String>,
    text: String,
    reasoning: String,
    tools: BTreeMap<u64, ChatToolState>,
    finish_reason: Option<String>,
    usage: Option<Value>,
}

impl OpenAiChatStreamDecoder {
    pub fn new(
        pin: LlmOperationExecutionPin,
        model_call_id: impl Into<String>,
    ) -> Result<Self, ShapeAdapterError> {
        if super::resolve_shape_adapter(&pin)?.key != ShapeAdapterKey::OpenAiChatCompletionsV1 {
            return Err(ShapeAdapterError::new(
                "adapter_execution_mismatch",
                "chat stream decoder does not match execution pin",
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
            finished: false,
            provider_id: None,
            model: None,
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            finish_reason: None,
            usage: None,
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        if self.finished {
            return Err(ShapeAdapterError::new(
                "stream_after_terminal",
                "chat stream chunk arrived after finish",
            ));
        }
        let value = parse_typed_terminal::<gproxy_protocol::openai::ChatCompletionChunk>(bytes)?;
        self.pin_provider_identity(&value)?;
        let mut batch = DecodedStreamBatch::default();
        if !self.started {
            self.started = true;
            batch.events.push(LlmStreamEventIr::Started {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
            });
        }
        if let Some(usage) = value.get("usage").filter(|value| !value.is_null()) {
            self.usage = Some(usage.clone());
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ShapeAdapterError::new("chat_choices_missing", "chat chunk choices are missing")
            })?;
        if choices.len() > 1 {
            return Err(ShapeAdapterError::new(
                "chat_choice_cardinality",
                "chat stream adapter accepts at most one choice per chunk",
            ));
        }
        let Some(choice) = choices.first() else {
            return Ok(batch);
        };
        if choice.get("index").and_then(Value::as_u64) != Some(0) {
            return Err(ShapeAdapterError::new(
                "chat_choice_index",
                "chat stream choice index must be zero",
            ));
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            if self
                .finish_reason
                .as_ref()
                .is_some_and(|existing| existing != reason)
            {
                return Err(ShapeAdapterError::new(
                    "chat_finish_reason_changed",
                    "chat stream finish reason changed",
                ));
            }
            self.finish_reason = Some(reason.into());
        }
        let delta = choice.get("delta").ok_or_else(|| {
            ShapeAdapterError::new("chat_delta_missing", "chat choice delta is missing")
        })?;
        if delta.get("function_call").is_some() {
            return Err(ShapeAdapterError::new(
                "unsupported_chat_stream_item",
                "legacy function_call streaming is unsupported",
            ));
        }
        if let Some(text) = delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            self.reasoning.push_str(text);
            batch.events.push(LlmStreamEventIr::ReasoningDelta {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                item_id: self.item_id("reasoning", 0),
                text: text.into(),
            });
        }
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .or_else(|| delta.get("refusal").and_then(Value::as_str))
            .filter(|text| !text.is_empty())
        {
            self.text.push_str(text);
            batch.events.push(LlmStreamEventIr::TextDelta {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                item_id: self.item_id("message", 0),
                text: text.into(),
            });
        }
        if let Some(tool_deltas) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool in tool_deltas {
                self.push_tool_delta(tool, &mut batch)?;
            }
        }
        Ok(batch)
    }

    pub fn finish(&mut self) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        if self.finished {
            return Err(ShapeAdapterError::new(
                "duplicate_stream_terminal",
                "chat stream was finished more than once",
            ));
        }
        if !self.started {
            return Err(ShapeAdapterError::new(
                "stream_start_missing",
                "chat stream ended before any chunk",
            ));
        }
        let finish_reason = self.finish_reason.as_deref().ok_or_else(|| {
            ShapeAdapterError::new(
                "stream_terminal_missing",
                "chat stream ended without finish reason",
            )
        })?;
        let mut tool_calls = Vec::new();
        let mut completed_items = Vec::new();
        for (index, state) in &self.tools {
            let provider_id = state.provider_id.as_deref().ok_or_else(|| {
                ShapeAdapterError::new("chat_tool_call_id_missing", "stream tool id is missing")
            })?;
            let name = state.name.as_deref().ok_or_else(|| {
                ShapeAdapterError::new("chat_tool_name_missing", "stream tool name is missing")
            })?;
            let arguments: Value = serde_json::from_str(&state.arguments).map_err(|_| {
                ShapeAdapterError::new(
                    "chat_tool_arguments_invalid",
                    "stream tool arguments are not complete JSON",
                )
            })?;
            tool_calls.push(json!({
                "type":"function",
                "id":provider_id,
                "function":{"name":name,"arguments":state.arguments},
            }));
            completed_items.push((*index, provider_id.to_owned(), name.to_owned(), arguments));
        }
        let terminal = json!({
            "id":self.provider_id.as_deref().unwrap_or("chat-stream"),
            "model":self.model.as_deref().unwrap_or(&self.pin.model_id),
            "choices":[{
                "index":0,
                "finish_reason":finish_reason,
                "message":{
                    "role":"assistant",
                    "content":(!self.text.is_empty()).then_some(self.text.as_str()),
                    "reasoning_content":(!self.reasoning.is_empty()).then_some(self.reasoning.as_str()),
                    "tool_calls":(!tool_calls.is_empty()).then_some(tool_calls),
                }
            }],
            "usage":self.usage,
        });
        let decoded = decode_openai_chat_terminal(
            &self.pin,
            &self.model_call_id,
            &serde_json::to_vec(&terminal).map_err(|_| {
                ShapeAdapterError::new(
                    "wire_terminal_decode_failed",
                    "chat stream terminal cannot be serialized",
                )
            })?,
        )?;
        let mut batch = DecodedStreamBatch::default();
        for (index, provider_id, name, arguments) in completed_items {
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
        self.finished = true;
        Ok(batch)
    }

    fn push_tool_delta(
        &mut self,
        value: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        let index = value.get("index").and_then(Value::as_u64).ok_or_else(|| {
            ShapeAdapterError::new("chat_tool_index_missing", "stream tool index is missing")
        })?;
        if value.get("custom").is_some() {
            return Err(ShapeAdapterError::new(
                "unsupported_chat_stream_item",
                "custom tool streaming is unsupported",
            ));
        }
        if !self.tools.contains_key(&index) && index != self.tools.len() as u64 {
            return Err(ShapeAdapterError::new(
                "chat_tool_index_gap",
                "chat stream tool indexes must be contiguous from zero",
            ));
        }
        let state = self.tools.entry(index).or_default();
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            set_once(&mut state.provider_id, id, "chat_tool_call_id_changed")?;
        }
        let function = value.get("function");
        let name = function
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str);
        if let Some(name) = name {
            set_once(&mut state.name, name, "chat_tool_name_changed")?;
        }
        let arguments = function
            .and_then(|value| value.get("arguments"))
            .and_then(Value::as_str);
        if let Some(arguments) = arguments {
            state.arguments.push_str(arguments);
        }
        batch.events.push(LlmStreamEventIr::ToolCallDelta {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
            item_id: self.item_id("tool", index),
            tool_call_id: self.item_id("call", index),
            name: name.map(str::to_owned),
            arguments_delta: arguments
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
        });
        Ok(())
    }

    fn pin_provider_identity(&mut self, value: &Value) -> Result<(), ShapeAdapterError> {
        let id = value.get("id").and_then(Value::as_str).ok_or_else(|| {
            ShapeAdapterError::new("chat_stream_id_missing", "chat stream id is missing")
        })?;
        let model = value.get("model").and_then(Value::as_str).ok_or_else(|| {
            ShapeAdapterError::new("chat_stream_model_missing", "chat stream model is missing")
        })?;
        set_once(&mut self.provider_id, id, "chat_stream_id_changed")?;
        set_once(&mut self.model, model, "chat_stream_model_changed")
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

fn set_once(
    slot: &mut Option<String>,
    value: &str,
    code: &'static str,
) -> Result<(), ShapeAdapterError> {
    if slot.as_ref().is_some_and(|existing| existing != value) {
        Err(ShapeAdapterError::new(
            code,
            "provider stream identity changed",
        ))
    } else {
        *slot = Some(value.into());
        Ok(())
    }
}
