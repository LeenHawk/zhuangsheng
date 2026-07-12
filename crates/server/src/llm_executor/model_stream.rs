use std::future::poll_fn;

use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    canonical,
    graph::LlmNodeExecutionSnapshot,
    llm::{
        EffectAttemptFence, LlmLoopCheckpoint, LlmRequestBuildOutput,
        adapter::{DecodedTerminalDraft, GenerationStreamDecoder, WireGenerationRequest},
        ir::StreamTerminal,
    },
    scheduler::ClaimedAttempt,
};

use crate::llm_executor_support::provider_error_bytes;

use super::{
    LocalLlmExecutor,
    hosted_tools::bind_hosted_stream_events,
    model_call::ModelCallResult,
    model_completion::{CompletedResponseInput, finish_decoded_model_call},
    model_stream_batch::StreamState,
    model_stream_failure::{fail_protocol, fail_stream},
};

pub(super) struct StreamModelCallInput<'a> {
    pub attempt: &'a ClaimedAttempt,
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub built: LlmRequestBuildOutput,
    pub wire: WireGenerationRequest,
    pub model_call_id: String,
    pub effect_attempt_id: String,
    pub checkpoint: LlmLoopCheckpoint,
    pub fence: EffectAttemptFence,
    pub credential: Option<&'a SecretValue>,
    pub now: i64,
}

pub(super) async fn execute_stream_model_call(
    executor: &LocalLlmExecutor,
    input: StreamModelCallInput<'_>,
) -> Result<ModelCallResult, ApplicationError> {
    let StreamModelCallInput {
        attempt,
        execution,
        built,
        wire,
        model_call_id,
        effect_attempt_id,
        checkpoint,
        fence,
        credential,
        now,
    } = input;
    let config = execution
        .streaming
        .as_ref()
        .ok_or(ApplicationError::Internal)?;
    let response = match executor
        .provider
        .send_stream(&execution.channel, &wire, credential)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return fail_stream(
                executor,
                effect_attempt_id,
                fence,
                checkpoint,
                error.code,
                &error.safe_message,
                provider_error_bytes(&error),
                now,
            )
            .await;
        }
    };
    let mut decoder =
        match GenerationStreamDecoder::new(execution.operation.clone(), model_call_id.clone()) {
            Ok(decoder) => decoder,
            Err(error) => {
                return fail_protocol(
                    executor,
                    effect_attempt_id,
                    fence,
                    checkpoint,
                    error.code,
                    &error.message,
                    now,
                )
                .await;
            }
        };
    let mut state = StreamState::new(config.persist_chunks);
    let mut frames = response.frames;
    while let Some(frame) = poll_fn(|context| frames.as_mut().poll_next(context)).await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                return fail_stream(
                    executor,
                    effect_attempt_id,
                    fence,
                    checkpoint,
                    error.code,
                    &error.safe_message,
                    provider_error_bytes(&error),
                    now,
                )
                .await;
            }
        };
        let mut batch = match decoder.push(&frame) {
            Ok(batch) => batch,
            Err(error) => {
                return fail_protocol(
                    executor,
                    effect_attempt_id,
                    fence,
                    checkpoint,
                    error.code,
                    &error.message,
                    now,
                )
                .await;
            }
        };
        if let Err(error) =
            bind_hosted_stream_events(&mut batch.events, &built.resolved_hosted_tools)
        {
            return fail_protocol(
                executor,
                effect_attempt_id,
                fence,
                checkpoint,
                error.code,
                error.message,
                now,
            )
            .await;
        }
        if let Err(error) = state
            .push_batch(
                executor,
                attempt,
                &model_call_id,
                &effect_attempt_id,
                &fence,
                config.audience,
                batch,
                now,
            )
            .await
        {
            return fail_protocol(
                executor,
                effect_attempt_id,
                fence,
                checkpoint,
                error.code,
                &error.message,
                now,
            )
            .await;
        }
    }
    let mut final_batch = match decoder.finish() {
        Ok(batch) => batch,
        Err(error) => {
            return fail_protocol(
                executor,
                effect_attempt_id,
                fence,
                checkpoint,
                error.code,
                &error.message,
                now,
            )
            .await;
        }
    };
    if let Err(error) =
        bind_hosted_stream_events(&mut final_batch.events, &built.resolved_hosted_tools)
    {
        return fail_protocol(
            executor,
            effect_attempt_id,
            fence,
            checkpoint,
            error.code,
            error.message,
            now,
        )
        .await;
    }
    if let Err(error) = state
        .push_batch(
            executor,
            attempt,
            &model_call_id,
            &effect_attempt_id,
            &fence,
            config.audience,
            final_batch,
            now,
        )
        .await
    {
        return fail_protocol(
            executor,
            effect_attempt_id,
            fence,
            checkpoint,
            error.code,
            &error.message,
            now,
        )
        .await;
    }
    state
        .flush(
            executor,
            attempt,
            &model_call_id,
            &effect_attempt_id,
            &fence,
            now,
        )
        .await?;
    let finalized = match state.finish() {
        Ok(finalized) => finalized,
        Err(error) => {
            return fail_protocol(
                executor,
                effect_attempt_id,
                fence,
                checkpoint,
                error.code,
                &error.message,
                now,
            )
            .await;
        }
    };
    match finalized.terminal {
        StreamTerminal::Completed(response) => {
            let response = *response;
            finish_decoded_model_call(
                executor,
                CompletedResponseInput {
                    operation: execution.operation.clone(),
                    built,
                    model_call_id,
                    effect_attempt_id,
                    checkpoint,
                    fence,
                    decoded: Ok(DecodedTerminalDraft {
                        response,
                        sensitive_entries: finalized.sensitive_entries,
                        opaque_attachments: finalized.opaque_attachments,
                    }),
                    now,
                },
            )
            .await
        }
        StreamTerminal::Failed(error) => {
            let code = error.code.as_deref().unwrap_or("provider_stream_failed");
            let bytes = canonical::to_vec(&error).map_err(|_| ApplicationError::Internal)?;
            fail_stream(
                executor,
                effect_attempt_id,
                fence,
                checkpoint,
                code,
                &error.message,
                bytes,
                now,
            )
            .await
        }
    }
}
