use std::collections::{HashMap, HashSet};

use serde_json::Value;
use thiserror::Error;

use crate::{canonical, compatibility::supports_operation_versions};

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub enum StreamTerminal {
    Completed(Box<LlmResponseIr>),
    Failed(LlmApiError),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct StreamProtocolError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct StreamFinalizer {
    call_id: Option<String>,
    next_seq: u64,
    started: bool,
    terminal: Option<StreamTerminal>,
    item_order: Vec<String>,
    seen_items: HashSet<String>,
    text: HashMap<String, String>,
    tool_deltas: HashMap<String, ToolDeltaState>,
}

#[derive(Debug, Default)]
struct ToolDeltaState {
    item_id: String,
    name: Option<String>,
    arguments: String,
    completed: bool,
}

impl StreamFinalizer {
    pub fn push(
        &mut self,
        event: LlmStreamEventIr,
    ) -> Result<Option<&StreamTerminal>, StreamProtocolError> {
        if self.terminal.is_some() {
            return error(
                "stream_after_terminal",
                "stream event arrived after terminal",
            );
        }
        if event.seq() != self.next_seq {
            return error(
                "stream_sequence_error",
                "normalized stream sequence is missing, duplicated, or out of order",
            );
        }
        if let Some(call_id) = &self.call_id {
            if event.call_id() != call_id {
                return error("stream_call_mismatch", "stream event changed model call id");
            }
        } else {
            self.call_id = Some(event.call_id().to_owned());
        }
        if !self.started && !matches!(event, LlmStreamEventIr::Started { .. }) {
            return error(
                "stream_start_missing",
                "first normalized event must be started",
            );
        }
        self.next_seq = self.next_seq.saturating_add(1);
        match event {
            LlmStreamEventIr::Started { .. } => {
                if self.started {
                    return error("duplicate_stream_start", "stream started more than once");
                }
                self.started = true;
            }
            LlmStreamEventIr::TextDelta { item_id, text, .. } => {
                if text.is_empty() || text.len() > 256 * 1024 {
                    return error("invalid_text_delta", "text delta is empty or too large");
                }
                self.observe(&item_id)?;
                let buffer = self.text.entry(item_id).or_default();
                if buffer.len().saturating_add(text.len()) > 4 * 1024 * 1024 {
                    return error("stream_text_limit", "stream text item exceeds four MiB");
                }
                buffer.push_str(&text);
            }
            LlmStreamEventIr::ReasoningDelta { item_id, text, .. } => {
                if text.is_empty() || text.len() > 256 * 1024 {
                    return error(
                        "invalid_reasoning_delta",
                        "reasoning delta is empty or too large",
                    );
                }
                self.observe(&item_id)?;
            }
            LlmStreamEventIr::ToolCallDelta {
                item_id,
                tool_call_id,
                name,
                arguments_delta,
                ..
            } => {
                self.observe(&item_id)?;
                let state = self.tool_deltas.entry(tool_call_id).or_default();
                if state.completed || !state.item_id.is_empty() && state.item_id != item_id {
                    return error(
                        "tool_delta_identity_error",
                        "tool call delta changed identity or continued after completion",
                    );
                }
                state.item_id = item_id;
                if let Some(name) = name {
                    if state
                        .name
                        .as_ref()
                        .is_some_and(|existing| existing != &name)
                    {
                        return error("tool_delta_name_error", "tool call name changed in stream");
                    }
                    state.name = Some(name);
                }
                if let Some(delta) = arguments_delta {
                    if state.arguments.len().saturating_add(delta.len()) > 256 * 1024 {
                        return error(
                            "tool_arguments_limit",
                            "stream tool arguments exceed 256 KiB",
                        );
                    }
                    state.arguments.push_str(&delta);
                }
            }
            LlmStreamEventIr::ToolCallCompleted { item, .. } => {
                let LlmTurnItemIr::AssistantToolCall { id, call } = item else {
                    return error(
                        "invalid_tool_terminal_item",
                        "tool_call_completed must contain an assistant tool call",
                    );
                };
                self.observe(&id)?;
                let state = self.tool_deltas.entry(call.id.clone()).or_default();
                if state.completed
                    || !state.item_id.is_empty() && state.item_id != id
                    || state.name.as_ref().is_some_and(|name| name != &call.name)
                {
                    return error(
                        "tool_terminal_mismatch",
                        "completed tool call does not match accumulated deltas",
                    );
                }
                if !state.arguments.is_empty() {
                    let parsed: Value = serde_json::from_str(&state.arguments).map_err(|_| {
                        StreamProtocolError {
                            code: "incomplete_tool_arguments",
                            message: "tool argument deltas do not form one JSON value".into(),
                        }
                    })?;
                    if canonical::to_vec(&parsed).ok() != canonical::to_vec(&call.arguments).ok() {
                        return error(
                            "tool_arguments_mismatch",
                            "completed tool arguments differ from accumulated deltas",
                        );
                    }
                }
                state.completed = true;
            }
            LlmStreamEventIr::HostedToolEvent { item, .. } => {
                let LlmTurnItemIr::HostedTool { id, .. } = item else {
                    return error(
                        "invalid_hosted_stream_item",
                        "hosted tool event must contain a hosted tool item",
                    );
                };
                self.observe(&id)?;
            }
            LlmStreamEventIr::Usage { usage, .. } => validate_usage_event(&usage)?,
            LlmStreamEventIr::Completed { response, .. } => {
                if response.model_call_id != self.call_id.as_deref().unwrap_or_default() {
                    return error(
                        "stream_terminal_call_mismatch",
                        "completed response has a different model call id",
                    );
                }
                if self.tool_deltas.values().any(|state| !state.completed) {
                    return error(
                        "incomplete_tool_call",
                        "stream completed with unfinished tool argument deltas",
                    );
                }
                validate_response_ir(&response).map_err(|error| StreamProtocolError {
                    code: error.code,
                    message: error.message,
                })?;
                self.validate_response_order_and_text(&response)?;
                self.terminal = Some(StreamTerminal::Completed(Box::new(response)));
            }
            LlmStreamEventIr::Failed { error: failure, .. } => {
                validate_failure(&failure)?;
                self.terminal = Some(StreamTerminal::Failed(failure));
            }
        }
        Ok(self.terminal.as_ref())
    }

    pub fn finish(self) -> Result<StreamTerminal, StreamProtocolError> {
        self.terminal.ok_or(StreamProtocolError {
            code: "stream_terminal_missing",
            message: "stream ended without a durable terminal".into(),
        })
    }

    fn observe(&mut self, item_id: &str) -> Result<(), StreamProtocolError> {
        if item_id.is_empty() || item_id.len() > 128 {
            return error("invalid_stream_item_id", "stream item id is invalid");
        }
        if self.seen_items.insert(item_id.into()) {
            self.item_order.push(item_id.into());
        }
        Ok(())
    }

    fn validate_response_order_and_text(
        &self,
        response: &LlmResponseIr,
    ) -> Result<(), StreamProtocolError> {
        let positions: HashMap<_, _> = response
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.id(), index))
            .collect();
        let mut previous = None;
        for item_id in &self.item_order {
            let Some(position) = positions.get(item_id.as_str()).copied() else {
                return error(
                    "stream_item_missing_from_terminal",
                    "observed stream item is absent from terminal response",
                );
            };
            if previous.is_some_and(|previous| position <= previous) {
                return error(
                    "stream_item_order_mismatch",
                    "terminal response changed first-observation item order",
                );
            }
            previous = Some(position);
        }
        for (item_id, expected) in &self.text {
            let Some(LlmTurnItemIr::Message { content, .. }) =
                response.items.iter().find(|item| item.id() == item_id)
            else {
                return error(
                    "stream_text_item_missing",
                    "text delta item is absent from terminal messages",
                );
            };
            let actual: String = content
                .iter()
                .filter_map(|part| match part {
                    LlmContentPartIr::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if &actual != expected {
                return error(
                    "stream_text_mismatch",
                    "terminal message text differs from accumulated deltas",
                );
            }
        }
        Ok(())
    }
}

fn validate_usage_event(usage: &LlmUsageIr) -> Result<(), StreamProtocolError> {
    if [
        usage.input_tokens,
        usage.output_tokens,
        usage.total_tokens,
        usage.cached_input_tokens,
        usage.reasoning_tokens,
    ]
    .into_iter()
    .flatten()
    .any(|value| value > 10_000_000_000)
    {
        return error("stream_usage_limit", "stream usage exceeds supported bound");
    }
    Ok(())
}

fn validate_failure(failure: &LlmApiError) -> Result<(), StreamProtocolError> {
    if !failure.operation_key.is_consistent()
        || !supports_operation_versions(
            failure.operation_taxonomy_version,
            failure.adapter_decoder_version,
        )
        || failure.message.is_empty()
        || failure.message.len() > 4096
        || failure.code.as_ref().is_some_and(|code| code.len() > 128)
    {
        return error(
            "invalid_stream_failure",
            "stream failure envelope is invalid",
        );
    }
    Ok(())
}

fn error<T>(code: &'static str, message: &str) -> Result<T, StreamProtocolError> {
    Err(StreamProtocolError {
        code,
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalizer_enforces_sequence_and_terminal_text() {
        let mut finalizer = StreamFinalizer::default();
        finalizer
            .push(LlmStreamEventIr::Started {
                call_id: "call-1".into(),
                seq: 0,
            })
            .unwrap();
        finalizer
            .push(LlmStreamEventIr::TextDelta {
                call_id: "call-1".into(),
                seq: 1,
                item_id: "message-1".into(),
                text: "hello".into(),
            })
            .unwrap();
        let response = LlmResponseIr {
            model_call_id: "call-1".into(),
            items: vec![LlmTurnItemIr::Message {
                id: "message-1".into(),
                role: MessageRole::Assistant,
                content: vec![LlmContentPartIr::Text {
                    text: "hello".into(),
                }],
                provenance: None,
            }],
            usage: None,
            finish_reason: Some(LlmFinishReason::Completed),
            continuation: None,
            raw_response_ref: None,
        };
        assert!(
            finalizer
                .push(LlmStreamEventIr::Completed {
                    call_id: "call-1".into(),
                    seq: 2,
                    response,
                })
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            finalizer.finish(),
            Ok(StreamTerminal::Completed(_))
        ));
    }

    #[test]
    fn finalizer_rejects_gap_and_missing_terminal() {
        let mut finalizer = StreamFinalizer::default();
        assert_eq!(
            finalizer
                .push(LlmStreamEventIr::Started {
                    call_id: "call-1".into(),
                    seq: 1,
                })
                .unwrap_err()
                .code,
            "stream_sequence_error"
        );
        assert_eq!(
            StreamFinalizer::default().finish().unwrap_err().code,
            "stream_terminal_missing"
        );
    }
}
