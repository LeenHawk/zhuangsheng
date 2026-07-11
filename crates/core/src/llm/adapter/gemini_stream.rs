use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    canonical,
    llm::{
        LlmOperationExecutionPin,
        ir::{LlmStreamEventIr, LlmTurnItemIr, ToolCallIr},
    },
};

use super::{
    DecodedStreamBatch, ShapeAdapterError, ShapeAdapterKey, common::parse_typed_terminal,
    decode_gemini_terminal,
};

pub struct GeminiStreamDecoder {
    pin: LlmOperationExecutionPin,
    model_call_id: String,
    next_seq: u64,
    started: bool,
    terminal: bool,
    parts: BTreeMap<u64, Value>,
    usage: Option<Value>,
}

impl GeminiStreamDecoder {
    pub fn new(
        pin: LlmOperationExecutionPin,
        model_call_id: impl Into<String>,
    ) -> Result<Self, ShapeAdapterError> {
        if super::resolve_shape_adapter(&pin)?.key != ShapeAdapterKey::GeminiGenerateContentV1 {
            return Err(ShapeAdapterError::new(
                "adapter_execution_mismatch",
                "Gemini stream decoder does not match execution pin",
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
            parts: BTreeMap::new(),
            usage: None,
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        if self.terminal {
            return Err(ShapeAdapterError::new(
                "stream_after_terminal",
                "Gemini stream chunk arrived after terminal",
            ));
        }
        let value =
            parse_typed_terminal::<gproxy_protocol::gemini::GenerateContentResponse>(bytes)?;
        let candidates = value
            .get("candidates")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                ShapeAdapterError::new("gemini_candidates_missing", "Gemini candidates are missing")
            })?;
        if candidates.len() != 1 {
            return Err(ShapeAdapterError::new(
                "gemini_candidate_cardinality",
                "Gemini stream requires exactly one candidate",
            ));
        }
        let candidate = &candidates[0];
        let mut batch = DecodedStreamBatch::default();
        if !self.started {
            self.started = true;
            batch.events.push(LlmStreamEventIr::Started {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
            });
        }
        if let Some(usage) = value.get("usageMetadata").filter(|value| !value.is_null()) {
            self.usage = Some(usage.clone());
        }
        if let Some(parts) = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
        {
            for (index, part) in parts.iter().enumerate() {
                self.push_part(index as u64, part, &mut batch)?;
            }
        }
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            self.push_terminal(reason, &value, &mut batch)?;
        }
        Ok(batch)
    }

    pub fn finish(&self) -> Result<(), ShapeAdapterError> {
        if self.terminal {
            Ok(())
        } else {
            Err(ShapeAdapterError::new(
                "stream_terminal_missing",
                "Gemini stream ended without a finish reason",
            ))
        }
    }

    fn push_part(
        &mut self,
        index: u64,
        part: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        if !self.parts.contains_key(&index) && index != self.parts.len() as u64 {
            return Err(ShapeAdapterError::new(
                "gemini_stream_part_index_gap",
                "Gemini stream part indexes must be contiguous from zero",
            ));
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let thought = part.get("thought").and_then(Value::as_bool) == Some(true)
                || part.get("thoughtSignature").is_some();
            let entry = self.parts.entry(index).or_insert_with(|| {
                if thought {
                    json!({"text":"","thought":true})
                } else {
                    json!({"text":""})
                }
            });
            if entry.get("text").is_none()
                || (entry.get("thought").and_then(Value::as_bool) == Some(true)) != thought
            {
                return Err(ShapeAdapterError::new(
                    "gemini_stream_part_identity_changed",
                    "Gemini stream part changed semantic type",
                ));
            }
            let buffer = entry.get("text").and_then(Value::as_str).ok_or_else(|| {
                ShapeAdapterError::new(
                    "gemini_stream_text_invalid",
                    "Gemini stream text state is invalid",
                )
            })?;
            let mut combined = buffer.to_owned();
            combined.push_str(text);
            entry["text"] = Value::String(combined);
            if let Some(signature) = part.get("thoughtSignature") {
                entry["thoughtSignature"] = signature.clone();
            }
            if !text.is_empty() {
                batch.events.push(if thought {
                    LlmStreamEventIr::ReasoningDelta {
                        call_id: self.model_call_id.clone(),
                        seq: self.take_seq(),
                        item_id: self.item_id("reasoning", index),
                        text: text.into(),
                    }
                } else {
                    LlmStreamEventIr::TextDelta {
                        call_id: self.model_call_id.clone(),
                        seq: self.take_seq(),
                        item_id: self.item_id("message", index),
                        text: text.into(),
                    }
                });
            }
        } else if let Some(call) = part.get("functionCall") {
            if let Some(existing) = self.parts.get(&index) {
                if existing != part {
                    return Err(ShapeAdapterError::new(
                        "gemini_stream_part_identity_changed",
                        "Gemini function call changed after first observation",
                    ));
                }
                return Ok(());
            }
            let name = call.get("name").and_then(Value::as_str).ok_or_else(|| {
                ShapeAdapterError::new("gemini_tool_name_missing", "Gemini tool name is missing")
            })?;
            let arguments = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let argument_text = canonical::to_string(&arguments).map_err(|_| {
                ShapeAdapterError::new(
                    "gemini_tool_arguments_invalid",
                    "Gemini tool arguments cannot be serialized",
                )
            })?;
            self.parts.insert(index, part.clone());
            batch.events.push(LlmStreamEventIr::ToolCallDelta {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                item_id: self.item_id("tool", index),
                tool_call_id: self.item_id("call", index),
                name: Some(name.into()),
                arguments_delta: Some(argument_text),
            });
            batch.events.push(LlmStreamEventIr::ToolCallCompleted {
                call_id: self.model_call_id.clone(),
                seq: self.take_seq(),
                item: LlmTurnItemIr::AssistantToolCall {
                    id: self.item_id("tool", index),
                    call: ToolCallIr {
                        id: self.item_id("call", index),
                        provider_call_id: call.get("id").and_then(Value::as_str).map(str::to_owned),
                        name: name.into(),
                        arguments,
                    },
                },
            });
        } else {
            if self.parts.insert(index, part.clone()).is_some() {
                return Err(ShapeAdapterError::new(
                    "gemini_stream_part_identity_changed",
                    "Gemini hosted part was repeated or replaced",
                ));
            }
        }
        Ok(())
    }

    fn push_terminal(
        &mut self,
        reason: &str,
        chunk: &Value,
        batch: &mut DecodedStreamBatch,
    ) -> Result<(), ShapeAdapterError> {
        let parts: Vec<_> = self.parts.values().cloned().collect();
        let terminal = json!({
            "candidates":[{
                "index":0,
                "finishReason":reason,
                "content":{"role":"model","parts":parts},
            }],
            "usageMetadata":self.usage,
            "modelVersion":chunk.get("modelVersion"),
            "responseId":chunk.get("responseId"),
        });
        let decoded = decode_gemini_terminal(
            &self.pin,
            &self.model_call_id,
            &serde_json::to_vec(&terminal).map_err(|_| {
                ShapeAdapterError::new(
                    "wire_terminal_decode_failed",
                    "Gemini stream terminal cannot be serialized",
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

    fn item_id(&self, kind: &str, index: u64) -> String {
        format!("{}:{kind}:{index}", self.model_call_id)
    }

    fn take_seq(&mut self) -> u64 {
        let value = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        value
    }
}
