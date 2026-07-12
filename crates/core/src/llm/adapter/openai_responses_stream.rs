use std::collections::BTreeSet;

use serde_json::Value;

use crate::llm::{
    LlmOperationExecutionPin,
    ir::{HostedToolPhase, LlmApiError, LlmStreamEventIr, LlmTurnItemIr, ToolCallIr},
};

use super::{
    DecodedStreamBatch, ShapeAdapterError, ShapeAdapterKey, common::parse_typed_terminal,
    decode_openai_responses_terminal,
};

pub struct OpenAiResponsesStreamDecoder {
    pin: LlmOperationExecutionPin,
    model_call_id: String,
    next_seq: u64,
    last_provider_seq: Option<u64>,
    started: bool,
    terminal: bool,
    output_indexes: BTreeSet<u64>,
}

impl OpenAiResponsesStreamDecoder {
    pub fn new(
        pin: LlmOperationExecutionPin,
        model_call_id: impl Into<String>,
    ) -> Result<Self, ShapeAdapterError> {
        if super::resolve_shape_adapter(&pin)?.key != ShapeAdapterKey::OpenAiResponsesV1 {
            return Err(ShapeAdapterError::new(
                "adapter_execution_mismatch",
                "responses stream decoder does not match execution pin",
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
            last_provider_seq: None,
            started: false,
            terminal: false,
            output_indexes: BTreeSet::new(),
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        if self.terminal {
            return Err(ShapeAdapterError::new(
                "stream_after_terminal",
                "responses stream event arrived after terminal",
            ));
        }
        let value = parse_typed_terminal::<gproxy_protocol::openai::ResponseStreamEvent>(bytes)?;
        self.validate_provider_sequence(value.get("sequence_number").and_then(Value::as_u64))?;
        let kind = value.get("type").and_then(Value::as_str).ok_or_else(|| {
            ShapeAdapterError::new(
                "responses_stream_type_missing",
                "responses stream event type is missing",
            )
        })?;
        let mut batch = DecodedStreamBatch::default();
        match kind {
            "response.created" => {
                if self.started {
                    return Err(ShapeAdapterError::new(
                        "duplicate_stream_start",
                        "responses stream started more than once",
                    ));
                }
                self.started = true;
                batch.events.push(self.started_event());
            }
            "response.output_item.added" => {
                self.require_started()?;
                let index = output_index(&value)?;
                self.observe_output_index(index)?;
                if value.pointer("/item/type").and_then(Value::as_str) == Some("function_call") {
                    batch.events.push(
                        self.tool_delta(
                            index,
                            value
                                .pointer("/item/name")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            None,
                        ),
                    );
                }
            }
            "response.output_text.delta" => {
                self.require_started()?;
                let index = output_index(&value)?;
                self.require_output_index(index)?;
                let text = required_string(&value, "delta", "responses_text_delta_missing")?;
                batch.events.push(LlmStreamEventIr::TextDelta {
                    call_id: self.model_call_id.clone(),
                    seq: self.take_seq(),
                    item_id: self.item_id("message", index),
                    text: text.into(),
                });
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                self.require_started()?;
                let index = output_index(&value)?;
                self.require_output_index(index)?;
                let text = required_string(&value, "delta", "responses_reasoning_delta_missing")?;
                batch.events.push(LlmStreamEventIr::ReasoningDelta {
                    call_id: self.model_call_id.clone(),
                    seq: self.take_seq(),
                    item_id: self.item_id("reasoning", index),
                    text: text.into(),
                });
            }
            "response.function_call_arguments.delta" => {
                self.require_started()?;
                let index = output_index(&value)?;
                self.require_output_index(index)?;
                let delta = required_string(&value, "delta", "responses_arguments_delta_missing")?;
                batch
                    .events
                    .push(self.tool_delta(index, None, Some(delta.into())));
            }
            "response.output_item.done" => {
                self.require_started()?;
                let item_kind = value.pointer("/item/type").and_then(Value::as_str);
                if item_kind == Some("function_call") {
                    let index = output_index(&value)?;
                    self.require_output_index(index)?;
                    let item = value.get("item").ok_or_else(|| {
                        ShapeAdapterError::new(
                            "responses_stream_item_missing",
                            "completed output item is missing",
                        )
                    })?;
                    batch.events.push(self.tool_completed(index, item)?);
                } else if item_kind == Some("web_search_call") {
                    let index = output_index(&value)?;
                    self.require_output_index(index)?;
                    let item = value.get("item").ok_or_else(|| {
                        ShapeAdapterError::new(
                            "responses_stream_item_missing",
                            "completed hosted output item is missing",
                        )
                    })?;
                    batch.events.push(self.hosted_completed(index, item)?);
                }
            }
            "response.completed" | "response.incomplete" => {
                self.require_started()?;
                let response = value.get("response").ok_or_else(|| {
                    ShapeAdapterError::new(
                        "responses_stream_terminal_missing",
                        "responses stream terminal object is missing",
                    )
                })?;
                let output_len = response
                    .get("output")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                if output_len != self.output_indexes.len() {
                    return Err(ShapeAdapterError::new(
                        "responses_stream_output_mismatch",
                        "responses terminal output does not match observed output items",
                    ));
                }
                let terminal = decode_openai_responses_terminal(
                    &self.pin,
                    &self.model_call_id,
                    &serde_json::to_vec(response).map_err(|_| {
                        ShapeAdapterError::new(
                            "wire_terminal_decode_failed",
                            "responses stream terminal cannot be serialized",
                        )
                    })?,
                )?;
                if let Some(usage) = terminal.response.usage.clone() {
                    batch.events.push(LlmStreamEventIr::Usage {
                        call_id: self.model_call_id.clone(),
                        seq: self.take_seq(),
                        usage,
                    });
                }
                batch.events.push(LlmStreamEventIr::Completed {
                    call_id: self.model_call_id.clone(),
                    seq: self.take_seq(),
                    response: terminal.response,
                });
                batch.sensitive_entries = terminal.sensitive_entries;
                batch.opaque_attachments = terminal.opaque_attachments;
                self.terminal = true;
            }
            "response.failed" => {
                self.require_started()?;
                batch.events.push(LlmStreamEventIr::Failed {
                    call_id: self.model_call_id.clone(),
                    seq: self.take_seq(),
                    error: self.failure(value.get("response")),
                });
                self.terminal = true;
            }
            _ => {}
        }
        Ok(batch)
    }

    fn validate_provider_sequence(&mut self, value: Option<u64>) -> Result<(), ShapeAdapterError> {
        let Some(value) = value else {
            return Ok(());
        };
        if self
            .last_provider_seq
            .is_some_and(|previous| value != previous.saturating_add(1))
        {
            return Err(ShapeAdapterError::new(
                "provider_stream_sequence_error",
                "responses provider sequence is duplicated, missing, or out of order",
            ));
        }
        self.last_provider_seq = Some(value);
        Ok(())
    }

    fn require_started(&self) -> Result<(), ShapeAdapterError> {
        if self.started {
            Ok(())
        } else {
            Err(ShapeAdapterError::new(
                "stream_start_missing",
                "responses stream emitted data before response.created",
            ))
        }
    }

    fn observe_output_index(&mut self, index: u64) -> Result<(), ShapeAdapterError> {
        if self.output_indexes.contains(&index) || index != self.output_indexes.len() as u64 {
            return Err(ShapeAdapterError::new(
                "responses_output_index_error",
                "responses output item indexes are duplicated or non-contiguous",
            ));
        }
        self.output_indexes.insert(index);
        Ok(())
    }

    fn require_output_index(&self, index: u64) -> Result<(), ShapeAdapterError> {
        if self.output_indexes.contains(&index) {
            Ok(())
        } else {
            Err(ShapeAdapterError::new(
                "responses_output_item_missing",
                "responses delta arrived before output_item.added",
            ))
        }
    }

    fn started_event(&mut self) -> LlmStreamEventIr {
        LlmStreamEventIr::Started {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
        }
    }

    fn tool_delta(
        &mut self,
        index: u64,
        name: Option<String>,
        arguments_delta: Option<String>,
    ) -> LlmStreamEventIr {
        LlmStreamEventIr::ToolCallDelta {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
            item_id: self.item_id("tool", index),
            tool_call_id: self.item_id("call", index),
            name,
            arguments_delta,
        }
    }

    fn tool_completed(
        &mut self,
        index: u64,
        item: &Value,
    ) -> Result<LlmStreamEventIr, ShapeAdapterError> {
        let provider_id = required_string(item, "call_id", "responses_call_id_missing")?;
        let name = required_string(item, "name", "responses_tool_name_missing")?;
        let raw = required_string(item, "arguments", "responses_tool_arguments_missing")?;
        let arguments = serde_json::from_str(raw).map_err(|_| {
            ShapeAdapterError::new(
                "responses_tool_arguments_invalid",
                "completed responses tool arguments are invalid JSON",
            )
        })?;
        Ok(LlmStreamEventIr::ToolCallCompleted {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
            item: LlmTurnItemIr::AssistantToolCall {
                id: self.item_id("tool", index),
                call: ToolCallIr {
                    id: self.item_id("call", index),
                    provider_call_id: Some(provider_id.into()),
                    name: name.into(),
                    arguments,
                },
            },
        })
    }

    fn hosted_completed(
        &mut self,
        index: u64,
        item: &Value,
    ) -> Result<LlmStreamEventIr, ShapeAdapterError> {
        let status = required_string(item, "status", "responses_hosted_status_missing")?;
        let phase = match status {
            "completed" => HostedToolPhase::Completed,
            "failed" => HostedToolPhase::Failed,
            "in_progress" | "searching" => HostedToolPhase::Running,
            _ => {
                return Err(ShapeAdapterError::new(
                    "responses_hosted_status_invalid",
                    "hosted tool status is unsupported",
                ));
            }
        };
        Ok(LlmStreamEventIr::HostedToolEvent {
            call_id: self.model_call_id.clone(),
            seq: self.take_seq(),
            item: LlmTurnItemIr::HostedTool {
                id: self.item_id("hosted", index),
                binding_id: "web_search_call".into(),
                kind: "web_search_call".into(),
                phase,
                display_content: Vec::new(),
                opaque_item_ref: None,
            },
        })
    }

    fn failure(&self, response: Option<&Value>) -> LlmApiError {
        let code = response
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str)
            .map(|value| value.chars().take(128).collect());
        LlmApiError {
            operation_key: self.pin.operation_key,
            operation_taxonomy_version: self.pin.operation_taxonomy_version,
            adapter_decoder_version: self.pin.adapter_decoder_version,
            status_code: None,
            code,
            message: "OpenAI Responses stream failed".into(),
            retryable: false,
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

fn output_index(value: &Value) -> Result<u64, ShapeAdapterError> {
    value
        .get("output_index")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            ShapeAdapterError::new(
                "responses_output_index_missing",
                "responses stream output index is missing",
            )
        })
}

fn required_string<'a>(
    value: &'a Value,
    field: &str,
    code: &'static str,
) -> Result<&'a str, ShapeAdapterError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ShapeAdapterError::new(code, "required responses stream field is missing"))
}
