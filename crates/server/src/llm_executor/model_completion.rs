use serde_json::json;
use zhuangsheng_core::{
    application::ApplicationError,
    llm::{
        EffectAttemptFence, FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint,
        LlmOperationExecutionPin, LlmRequestBuildOutput, ModelCallEffectOutcome,
        adapter::{DecodedTerminalDraft, OpaqueAttachmentTarget, ShapeAdapterError},
        ir::LlmTurnItemIr,
    },
};

use crate::llm_executor_support::{finalize_failure, set_model_status};

use super::{
    LocalLlmExecutor,
    hosted_tools::bind_hosted_response_items,
    model_call::{CompletedModelCall, ModelCallResult},
};

pub(super) struct CompletedResponseInput {
    pub operation: LlmOperationExecutionPin,
    pub built: LlmRequestBuildOutput,
    pub model_call_id: String,
    pub effect_attempt_id: String,
    pub checkpoint: LlmLoopCheckpoint,
    pub fence: EffectAttemptFence,
    pub decoded: Result<DecodedTerminalDraft, ShapeAdapterError>,
    pub now: i64,
}

pub(super) async fn finish_decoded_model_call(
    executor: &LocalLlmExecutor,
    input: CompletedResponseInput,
) -> Result<ModelCallResult, ApplicationError> {
    let CompletedResponseInput {
        built,
        operation,
        model_call_id,
        effect_attempt_id,
        mut checkpoint,
        fence,
        decoded,
        now,
    } = input;
    let mut decoded = decoded;
    let mut opaque_error = None;
    if let Ok(draft) = decoded.as_mut() {
        if draft.sensitive_entries.is_empty() != draft.opaque_attachments.is_empty() {
            opaque_error = Some((
                "llm_opaque_attachment_invalid",
                "provider opaque attachments do not match sensitive entries",
            ));
        } else if !draft.sensitive_entries.is_empty() {
            match executor
                .store
                .store_llm_opaque_bundle(
                    &effect_attempt_id,
                    &model_call_id,
                    &operation,
                    &draft.sensitive_entries,
                    now,
                )
                .await
            {
                Ok(references) => {
                    if attach_opaque_refs(draft, &references.entries).is_err() {
                        opaque_error = Some((
                            "llm_opaque_attachment_invalid",
                            "provider opaque attachment target is invalid",
                        ));
                    } else {
                        draft.sensitive_entries.clear();
                        draft.opaque_attachments.clear();
                    }
                }
                Err(_) => {
                    opaque_error = Some((
                        "llm_opaque_storage_failed",
                        "provider opaque continuation could not be stored securely",
                    ));
                }
            }
        }
        checkpoint.continuation_ref = draft.response.continuation.clone();
    }
    set_model_status(&mut checkpoint, LlmLogicalCallStatus::Completed);
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
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
                && opaque_error.is_none()
                && draft.sensitive_entries.is_empty()
                && draft.opaque_attachments.is_empty()
        })
        .map(|draft| {
            let mut transcript = built.request.transcript.clone();
            transcript.extend(draft.response.items.clone());
            transcript
        });
    let persisted_transcript = durable_transcript.clone();
    let response_bytes = durable_response_bytes(&decoded, hosted_error.as_ref(), opaque_error)?;
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
    if let Some((code, message)) = opaque_error {
        return Ok(ModelCallResult::Terminal(finalize_failure(code, message)));
    }
    Ok(ModelCallResult::Completed(Box::new(CompletedModelCall {
        model_call_id,
        checkpoint,
        decoded,
        resolved_tools: built.resolved_tools,
        resolved_memory_tools: built.resolved_memory_tools,
        transcript: persisted_transcript.ok_or(ApplicationError::Internal)?,
    })))
}

fn durable_response_bytes(
    decoded: &Result<DecodedTerminalDraft, ShapeAdapterError>,
    hosted_error: Option<&super::hosted_tools::HostedToolResponseError>,
    opaque_error: Option<(&'static str, &'static str)>,
) -> Result<Vec<u8>, ApplicationError> {
    let value = match decoded {
        Err(error) => json!({
            "schemaVersion":1,
            "kind":"normalized_response_rejected",
            "errorCode":error.code,
        }),
        Ok(_) if hosted_error.is_some() => json!({
            "schemaVersion":1,
            "kind":"normalized_response_rejected",
            "errorCode":hosted_error.unwrap().code,
        }),
        Ok(_) if opaque_error.is_some() => json!({
            "schemaVersion":1,
            "kind":"normalized_response_rejected",
            "errorCode":opaque_error.unwrap().0,
        }),
        Ok(draft) => json!({
            "schemaVersion":1,
            "kind":"normalized_response",
            "response":draft.response,
        }),
    };
    zhuangsheng_core::canonical::to_vec(&value).map_err(|_| ApplicationError::Internal)
}

fn attach_opaque_refs(
    draft: &mut DecodedTerminalDraft,
    references: &std::collections::BTreeMap<
        String,
        zhuangsheng_core::llm::ir::OpaqueContinuationRef,
    >,
) -> Result<(), ()> {
    if draft.opaque_attachments.len() != references.len() {
        return Err(());
    }
    let mut attached = std::collections::BTreeSet::new();
    for attachment in &draft.opaque_attachments {
        let reference = references.get(&attachment.entry_key).ok_or(())?.clone();
        if !attached.insert(&attachment.entry_key) {
            return Err(());
        }
        match &attachment.target {
            OpaqueAttachmentTarget::ResponseContinuation => {
                if draft.response.continuation.replace(reference).is_some() {
                    return Err(());
                }
            }
            OpaqueAttachmentTarget::Item { item_id } => {
                let item = draft
                    .response
                    .items
                    .iter_mut()
                    .find(|item| item.id() == item_id)
                    .ok_or(())?;
                match item {
                    LlmTurnItemIr::HostedTool {
                        opaque_item_ref, ..
                    }
                    | LlmTurnItemIr::Reasoning {
                        opaque_item_ref, ..
                    } => {
                        if opaque_item_ref.replace(reference).is_some() {
                            return Err(());
                        }
                    }
                    _ => return Err(()),
                }
            }
        }
    }
    Ok(())
}
