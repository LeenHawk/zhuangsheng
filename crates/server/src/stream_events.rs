use serde::Serialize;
use tokio::sync::broadcast;
use zhuangsheng_core::{graph::StreamingAudience, llm::ir::LlmStreamEventIr};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EphemeralLlmStreamEvent {
    pub schema_version: u32,
    pub run_id: String,
    pub node_instance_id: String,
    pub attempt_id: String,
    pub model_call_id: String,
    pub audience: StreamingAudience,
    pub event: LlmStreamEventIr,
}

impl EphemeralLlmStreamEvent {
    pub fn event_type(&self) -> &'static str {
        match self.event {
            LlmStreamEventIr::Started { .. } => "llm.stream.started",
            LlmStreamEventIr::TextDelta { .. } => "llm.stream.text_delta",
            LlmStreamEventIr::ReasoningDelta { .. } => "llm.stream.reasoning_delta",
            LlmStreamEventIr::ToolCallDelta { .. } => "llm.stream.tool_call_delta",
            LlmStreamEventIr::ToolCallCompleted { .. } => "llm.stream.tool_call_completed",
            LlmStreamEventIr::HostedToolEvent { .. } => "llm.stream.hosted_tool",
            LlmStreamEventIr::Usage { .. } => "llm.stream.usage",
            LlmStreamEventIr::Completed { .. } => "llm.stream.completed",
            LlmStreamEventIr::Failed { .. } => "llm.stream.failed",
        }
    }
}

#[derive(Clone)]
pub struct StreamEventHub {
    sender: broadcast::Sender<EphemeralLlmStreamEvent>,
}

impl Default for StreamEventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamEventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self { sender }
    }

    pub fn publish(&self, event: EphemeralLlmStreamEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EphemeralLlmStreamEvent> {
        self.sender.subscribe()
    }
}
