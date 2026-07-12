use zhuangsheng_core::{
    application::ApplicationError,
    canonical,
    graph::StreamingAudience,
    llm::{
        EffectAttemptFence, PersistLlmStreamChunkCommand,
        adapter::{DecodedStreamBatch, OpaqueAttachmentDraft, SensitiveEntryDraft},
        ir::{LlmStreamEventIr, StreamFinalizer, StreamProtocolError, StreamTerminal},
    },
    scheduler::ClaimedAttempt,
};

use crate::EphemeralLlmStreamEvent;

use super::LocalLlmExecutor;

const PERSIST_EVENT_LIMIT: usize = 32;
const PERSIST_BYTE_TARGET: usize = 16 * 1024;

pub(super) struct FinalizedStream {
    pub terminal: StreamTerminal,
    pub sensitive_entries: Vec<SensitiveEntryDraft>,
    pub opaque_attachments: Vec<OpaqueAttachmentDraft>,
}

pub(super) struct StreamState {
    finalizer: StreamFinalizer,
    sensitive_entries: Vec<SensitiveEntryDraft>,
    opaque_attachments: Vec<OpaqueAttachmentDraft>,
    persist: bool,
    pending: Vec<LlmStreamEventIr>,
    pending_bytes: usize,
    chunk_no: u64,
}

impl StreamState {
    pub fn new(persist: bool) -> Self {
        Self {
            finalizer: StreamFinalizer::default(),
            sensitive_entries: Vec::new(),
            opaque_attachments: Vec::new(),
            persist,
            pending: Vec::new(),
            pending_bytes: 0,
            chunk_no: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn push_batch(
        &mut self,
        executor: &LocalLlmExecutor,
        attempt: &ClaimedAttempt,
        model_call_id: &str,
        effect_attempt_id: &str,
        fence: &EffectAttemptFence,
        audience: StreamingAudience,
        batch: DecodedStreamBatch,
        now: i64,
    ) -> Result<(), StreamProtocolError> {
        self.sensitive_entries.extend(batch.sensitive_entries);
        self.opaque_attachments.extend(batch.opaque_attachments);
        for event in batch.events {
            publish(executor, attempt, model_call_id, audience, &event);
            if self.persist && !event.is_terminal() {
                self.pending_bytes = self
                    .pending_bytes
                    .saturating_add(canonical::to_vec(&event).map_or(0, |bytes| bytes.len()));
                self.pending.push(event.clone());
            }
            self.finalizer.push(event)?;
            if self.pending.len() >= PERSIST_EVENT_LIMIT
                || self.pending_bytes >= PERSIST_BYTE_TARGET
            {
                self.flush(
                    executor,
                    attempt,
                    model_call_id,
                    effect_attempt_id,
                    fence,
                    now,
                )
                .await
                .map_err(|_| StreamProtocolError {
                    code: "stream_chunk_persist_failed",
                    message: "stream chunk could not be persisted".into(),
                })?;
            }
        }
        Ok(())
    }

    pub async fn flush(
        &mut self,
        executor: &LocalLlmExecutor,
        attempt: &ClaimedAttempt,
        model_call_id: &str,
        effect_attempt_id: &str,
        fence: &EffectAttemptFence,
        now: i64,
    ) -> Result<(), ApplicationError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.chunk_no = self
            .chunk_no
            .checked_add(1)
            .ok_or(ApplicationError::Internal)?;
        executor
            .store
            .persist_llm_stream_chunk(
                PersistLlmStreamChunkCommand {
                    node_instance_id: attempt.node_instance_id.clone(),
                    model_call_id: model_call_id.into(),
                    effect_attempt_id: effect_attempt_id.into(),
                    chunk_no: self.chunk_no,
                    fence: fence.clone(),
                    events: std::mem::take(&mut self.pending),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        self.pending_bytes = 0;
        Ok(())
    }

    pub fn finish(self) -> Result<FinalizedStream, StreamProtocolError> {
        Ok(FinalizedStream {
            terminal: self.finalizer.finish()?,
            sensitive_entries: self.sensitive_entries,
            opaque_attachments: self.opaque_attachments,
        })
    }
}

fn publish(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    model_call_id: &str,
    audience: StreamingAudience,
    event: &LlmStreamEventIr,
) {
    let visible = match audience {
        StreamingAudience::Internal => false,
        StreamingAudience::User => matches!(
            event,
            LlmStreamEventIr::Started { .. }
                | LlmStreamEventIr::TextDelta { .. }
                | LlmStreamEventIr::Failed { .. }
        ),
        StreamingAudience::Trace | StreamingAudience::Both => true,
    };
    if visible {
        executor.stream_events.publish(EphemeralLlmStreamEvent {
            schema_version: 1,
            run_id: attempt.run_id.clone(),
            node_instance_id: attempt.node_instance_id.clone(),
            attempt_id: attempt.attempt_id.clone(),
            model_call_id: model_call_id.into(),
            audience,
            event: event.clone(),
        });
    }
}
