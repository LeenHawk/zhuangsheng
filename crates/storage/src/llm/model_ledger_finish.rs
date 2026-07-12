use sea_orm::TransactionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        FinishModelCallCommand, LlmLogicalCallStatus, ModelCallEffectOutcome,
        ir::validate_transcript_ir,
    },
};

use crate::{SqliteStore, StorageError, StorageResult};

use super::{
    effect_wait::{
        EffectWait, EffectWaitOwner, allocate_wait_id, load_wait_ids, open_effect_resolution_wait,
    },
    model_ledger_helpers::{add_ref, finish_rows, persist_checkpoint},
    model_ledger_outcome::store_outcome,
    model_ledger_replay::{ReplayDecision, classify_finish},
    validation::{
        CheckpointExpectation, load_ledger_context, load_model_call_attempt, validate_active_fence,
        validate_checkpoint, validate_replay_fence,
    },
};

impl SqliteStore {
    pub async fn finish_model_call(
        &self,
        mut command: FinishModelCallCommand,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let fenced =
            load_model_call_attempt(&transaction, &command.effect_attempt_id, &command.fence)
                .await?;
        let context = load_ledger_context(
            &transaction,
            &fenced.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        let mut checkpoint = command.checkpoint;
        let durable_wait_ids = load_wait_ids(&transaction, &fenced.node_instance_id).await?;
        if durable_wait_ids.is_empty() {
            if !checkpoint.wait_ids.is_empty() {
                return Err(StorageError::InvalidArgument(
                    "LLM finish checkpoint contains unknown wait ids".into(),
                ));
            }
        } else {
            checkpoint.wait_ids = durable_wait_ids;
        }
        if let Some(active) = &mut checkpoint.active_model_effect {
            active.response_ref = fenced.response_object_id.clone();
        }
        let transcript = command.transcript.take();
        if transcript.is_some()
            && !matches!(command.outcome, ModelCallEffectOutcome::Completed { .. })
        {
            return Err(StorageError::InvalidArgument(
                "only a completed model call can append transcript items".into(),
            ));
        }
        if let Some(items) = transcript {
            validate_transcript_ir(&items).map_err(|error| {
                StorageError::InvalidArgument(format!(
                    "invalid durable LLM transcript: {}",
                    error.message
                ))
            })?;
            checkpoint.transcript_ref = crate::graph::helpers::put_inline_object(
                &transaction,
                &canonical::to_vec(&json!({"schemaVersion":1,"items":items}))?,
                now,
            )
            .await?;
        }
        checkpoint = checkpoint.seal()?;
        let replay = classify_finish(&fenced, &command.outcome, &checkpoint.checksum)?;
        if matches!(replay.decision, ReplayDecision::Replayed) {
            validate_replay_fence(&fenced, &command.fence)?;
            validate_checkpoint(
                &checkpoint,
                CheckpointExpectation {
                    context: &context,
                    node_instance_id: &fenced.node_instance_id,
                    updater_attempt_id: &command.fence.invoking_node_attempt_id,
                    call_no: fenced.call_no,
                    model_call_id: &fenced.model_call_id,
                    effect_id: &fenced.effect_id,
                    effect_attempt_id: &command.effect_attempt_id,
                    status: replay.logical_status,
                    response_ref: replay.response_ref.as_deref(),
                },
            )?;
            transaction.commit().await?;
            return Ok(());
        }
        validate_active_fence(&fenced, &command.fence)?;
        let stored = store_outcome(&transaction, &command.outcome, now).await?;
        if stored.logical_status == LlmLogicalCallStatus::RetryReady
            && fenced.classification == "non_idempotent"
        {
            return Err(StorageError::InvalidArgument(
                "non-idempotent started effect cannot become retry-ready".into(),
            ));
        }
        let wait_id = if stored.logical_status == LlmLogicalCallStatus::OutcomeUnknown {
            let wait_id = allocate_wait_id(&transaction, &fenced.node_instance_id).await?;
            checkpoint.wait_ids.push(wait_id.clone());
            Some(wait_id)
        } else {
            None
        };
        if let Some(active) = &mut checkpoint.active_model_effect {
            active.response_ref = stored.result_object_id.clone();
        }
        checkpoint = checkpoint.seal()?;
        validate_checkpoint(
            &checkpoint,
            CheckpointExpectation {
                context: &context,
                node_instance_id: &fenced.node_instance_id,
                updater_attempt_id: &command.fence.invoking_node_attempt_id,
                call_no: fenced.call_no,
                model_call_id: &fenced.model_call_id,
                effect_id: &fenced.effect_id,
                effect_attempt_id: &command.effect_attempt_id,
                status: stored.logical_status,
                response_ref: stored.result_object_id.as_deref(),
            },
        )?;
        finish_rows(
            &transaction,
            &fenced,
            &command.effect_attempt_id,
            &stored,
            now,
        )
        .await?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        if let Some(wait_id) = &wait_id {
            open_effect_resolution_wait(
                &transaction,
                EffectWait {
                    wait_id,
                    node_instance_id: &fenced.node_instance_id,
                    invoking_node_attempt_id: &command.fence.invoking_node_attempt_id,
                    owner: EffectWaitOwner::Model {
                        model_call_id: &fenced.model_call_id,
                    },
                    effect_id: &fenced.effect_id,
                    effect_attempt_id: &command.effect_attempt_id,
                    classification: &fenced.classification,
                },
                now,
            )
            .await?;
        }
        if let Some(object_id) = &stored.result_object_id {
            add_ref(
                &transaction,
                object_id,
                "model_call",
                &fenced.model_call_id,
                "response",
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}
