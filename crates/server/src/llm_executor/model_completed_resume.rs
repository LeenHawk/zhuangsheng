use std::collections::BTreeSet;

use zhuangsheng_core::{
    graph::LlmNodeExecutionSnapshot,
    llm::{
        LlmRequestBuildInput, LlmResumeState,
        adapter::DecodedTerminalDraft,
        build_llm_request,
        context::ContextAssemblyOutput,
        ir::{LlmFinishReason, LlmResponseIr, LlmTurnItemIr},
    },
};

use super::model_call::CompletedModelCall;

#[cfg(test)]
pub(crate) struct CompletedModelPause {
    armed: std::sync::atomic::AtomicBool,
    pub started: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}

#[cfg(test)]
impl CompletedModelPause {
    pub fn new() -> Self {
        Self {
            armed: std::sync::atomic::AtomicBool::new(true),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        }
    }

    pub async fn wait_once(&self) {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
    }
}

pub(super) struct CompletedResumeError {
    pub code: &'static str,
    pub message: String,
}

pub(super) fn reconstruct_completed_model_call(
    execution: &LlmNodeExecutionSnapshot,
    context: &ContextAssemblyOutput,
    base_transcript_len: usize,
    state: LlmResumeState,
) -> Result<CompletedModelCall, CompletedResumeError> {
    let model_call_id = state
        .checkpoint
        .active_model_effect
        .as_ref()
        .map(|active| active.model_call_id.clone())
        .ok_or_else(|| {
            error(
                "llm_completed_resume_invalid",
                "active model call is missing",
            )
        })?;
    let prefix = format!("{model_call_id}:");
    let response_start = state
        .transcript
        .iter()
        .enumerate()
        .skip(base_transcript_len)
        .find_map(|(index, item)| item.id().starts_with(&prefix).then_some(index))
        .ok_or_else(|| {
            error(
                "llm_completed_resume_invalid",
                "completed response items are absent from the durable transcript",
            )
        })?;
    if state.transcript[response_start..]
        .iter()
        .any(|item| !item.id().starts_with(&prefix))
    {
        return Err(error(
            "llm_completed_resume_invalid",
            "completed response is not the terminal transcript suffix",
        ));
    }
    let request_tail = &state.transcript[base_transcript_len..response_start];
    let built = build_llm_request(LlmRequestBuildInput {
        execution,
        context,
        registry_snapshot: &execution.tool_registry,
        tool_descriptors: &execution.tool_descriptors,
        transcript_tail: request_tail,
        continuation: state.checkpoint.continuation_ref.as_ref(),
        approved_hosted_bindings: &BTreeSet::new(),
        model_call_no: state.checkpoint.model_call_no,
    })
    .map_err(|error| CompletedResumeError {
        code: error.code,
        message: error.message,
    })?;
    let items: Vec<LlmTurnItemIr> = state.transcript[response_start..].to_vec();
    let finish_reason = if items
        .iter()
        .any(|item| matches!(item, LlmTurnItemIr::AssistantToolCall { .. }))
    {
        LlmFinishReason::ToolCalls
    } else {
        LlmFinishReason::Completed
    };
    Ok(CompletedModelCall {
        model_call_id: model_call_id.clone(),
        checkpoint: state.checkpoint,
        decoded: DecodedTerminalDraft {
            response: LlmResponseIr {
                model_call_id,
                items,
                usage: None,
                finish_reason: Some(finish_reason),
                continuation: None,
                raw_response_ref: None,
            },
            sensitive_entries: Vec::new(),
            opaque_attachments: Vec::new(),
        },
        resolved_tools: built.resolved_tools,
        resolved_memory_tools: built.resolved_memory_tools,
        transcript: state.transcript,
    })
}

fn error(code: &'static str, message: &str) -> CompletedResumeError {
    CompletedResumeError {
        code,
        message: message.into(),
    }
}
