use zhuangsheng_core::{
    application::ApplicationError,
    llm::{
        EffectAttemptFence, FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint,
        LlmRequestBuildOutput, ModelCallEffectOutcome,
        adapter::{DecodedTerminalDraft, ShapeAdapterError},
    },
};

use crate::llm_executor_support::{finalize_failure, set_model_status};

use super::{
    LocalLlmExecutor,
    hosted_tools::bind_hosted_response_items,
    model_call::{CompletedModelCall, ModelCallResult},
};

pub(super) struct CompletedResponseInput {
    pub built: LlmRequestBuildOutput,
    pub model_call_id: String,
    pub effect_attempt_id: String,
    pub checkpoint: LlmLoopCheckpoint,
    pub fence: EffectAttemptFence,
    pub response_bytes: Vec<u8>,
    pub decoded: Result<DecodedTerminalDraft, ShapeAdapterError>,
    pub now: i64,
}

pub(super) async fn finish_decoded_model_call(
    executor: &LocalLlmExecutor,
    input: CompletedResponseInput,
) -> Result<ModelCallResult, ApplicationError> {
    let CompletedResponseInput {
        built,
        model_call_id,
        effect_attempt_id,
        mut checkpoint,
        fence,
        response_bytes,
        decoded,
        now,
    } = input;
    set_model_status(&mut checkpoint, LlmLogicalCallStatus::Completed);
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    let mut decoded = decoded;
    let hosted_error = decoded.as_mut().ok().and_then(|draft| {
        bind_hosted_response_items(&mut draft.response.items, &built.resolved_hosted_tools).err()
    });
    let usage = decoded
        .as_ref()
        .ok()
        .and_then(|draft| draft.response.usage.clone());
    let durable_transcript = decoded
        .as_ref()
        .ok()
        .filter(|draft| {
            hosted_error.is_none()
                && draft.sensitive_entries.is_empty()
                && draft.opaque_attachments.is_empty()
        })
        .map(|draft| {
            let mut transcript = built.request.transcript.clone();
            transcript.extend(draft.response.items.clone());
            transcript
        });
    let persisted_transcript = durable_transcript.clone();
    checkpoint = executor
        .store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id,
                fence,
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes,
                    usage,
                },
                checkpoint,
                transcript: durable_transcript,
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    let decoded = match decoded {
        Ok(value) => value,
        Err(error) => {
            return Ok(ModelCallResult::Terminal(finalize_failure(
                error.code,
                &error.message,
            )));
        }
    };
    if let Some(error) = hosted_error {
        return Ok(ModelCallResult::Terminal(finalize_failure(
            error.code,
            error.message,
        )));
    }
    if !decoded.sensitive_entries.is_empty() || !decoded.opaque_attachments.is_empty() {
        return Ok(ModelCallResult::Terminal(finalize_failure(
            "llm_opaque_storage_pending",
            "provider response requires opaque continuation storage",
        )));
    }
    Ok(ModelCallResult::Completed(Box::new(CompletedModelCall {
        model_call_id,
        checkpoint,
        decoded,
        resolved_tools: built.resolved_tools,
        transcript: persisted_transcript.ok_or(ApplicationError::Internal)?,
    })))
}
