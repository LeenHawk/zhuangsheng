use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::EffectClassification,
    llm::{
        EffectAttemptStatus, EffectStatus, LlmLogicalCallStatus, PrepareCountCallCommand,
        PrepareCountCallRetryCommand, PreparedCountCall, StartCountCallCommand,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::{
    count_ledger_helpers::{add_count_refs, append_count_event, load_existing},
    count_ledger_retry::prepare_retry,
    count_validation::{
        CountCheckpointExpectation, load_count_attempt, validate_count_checkpoint,
        validate_count_fence, validate_count_pin,
    },
    model_ledger_helpers::{classification_name, persist_checkpoint},
    validation::load_ledger_context,
};

impl SqliteStore {
    pub async fn prepare_count_call(
        &self,
        command: PrepareCountCallCommand,
        now: i64,
    ) -> StorageResult<PreparedCountCall> {
        validate_prepare(&command)?;
        let transaction = self.db.begin().await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        let pin_digest = validate_count_pin(&context.snapshot, &command.pin, &command.channel_id)?;
        let candidate_digest = canonical::hash_bytes(&command.trim_candidate_bytes);
        let request_digest = canonical::hash_bytes(&command.request_bytes);
        let retry_json = canonical::to_string(&command.retry_policy)?;
        if let Some(existing) = load_existing(
            &transaction,
            &command,
            &pin_digest,
            &candidate_digest,
            &request_digest,
            &retry_json,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(existing);
        }
        let count: i64 = transaction
            .query_one_raw(sql(
                "SELECT COUNT(*) AS count FROM count_calls WHERE node_instance_id = ?",
                vec![command.node_instance_id.clone().into()],
            ))
            .await?
            .expect("count query returns a row")
            .try_get("", "count")?;
        let expected_ordinal = u64::try_from(count)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| StorageError::Integrity("invalid count-call count".into()))?;
        let limit = context
            .snapshot
            .limits
            .max_count_calls
            .ok_or_else(|| StorageError::Integrity("count-call limit is not pinned".into()))?;
        if command.count_ordinal != expected_ordinal || command.count_ordinal > limit {
            return Err(StorageError::InvalidArgument(
                "count ordinal is non-sequential or exceeds its limit".into(),
            ));
        }
        let candidate_ref =
            put_inline_object(&transaction, &command.trim_candidate_bytes, now).await?;
        let request_ref = put_inline_object(&transaction, &command.request_bytes, now).await?;
        let mut checkpoint = command.checkpoint;
        let active = checkpoint.active_count_effect.as_mut().ok_or_else(|| {
            StorageError::InvalidArgument("prepared count checkpoint is missing".into())
        })?;
        active.trim_candidate_ref = candidate_ref.clone();
        checkpoint = checkpoint.seal()?;
        validate_count_checkpoint(
            &checkpoint,
            CountCheckpointExpectation {
                context: &context,
                node_instance_id: &command.node_instance_id,
                updater_attempt_id: &command.originating_attempt_id,
                count_call_id: &command.count_call_id,
                effect_id: &command.effect_id,
                effect_attempt_id: &command.effect_attempt_id,
                count_ordinal: command.count_ordinal,
                pin_digest: &pin_digest,
                candidate_ref: &candidate_ref,
                candidate_digest: &candidate_digest,
                request_digest: &request_digest,
                status: LlmLogicalCallStatus::Prepared,
                result_source: None,
                result_ref: None,
            },
        )?;
        let operation_key = command
            .pin
            .provider_count_operation_key
            .unwrap_or(command.pin.generation_operation.operation_key);
        transaction
            .execute_raw(sql(
                "INSERT INTO count_calls (id, node_instance_id, originating_attempt_id, count_ordinal, channel_id, channel_revision_id, model_id, operation_key_json, operation_taxonomy_version, adapter_decoder_version, local_counter_id, local_counter_version, fallback_policy_version, safety_margin_tokens, count_execution_pin_digest, trim_candidate_object_id, trim_candidate_digest, request_digest, request_object_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'prepared', ?)",
                vec![
                    command.count_call_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    command.originating_attempt_id.clone().into(),
                    i64::try_from(command.count_ordinal).map_err(|_| StorageError::InvalidArgument("count ordinal is too large".into()))?.into(),
                    command.channel_id.clone().into(),
                    command.pin.generation_operation.channel_revision_id.clone().into(),
                    command.pin.generation_operation.model_id.clone().into(),
                    canonical::to_string(&operation_key)?.into(),
                    i64::from(command.pin.generation_operation.operation_taxonomy_version).into(),
                    i64::from(command.pin.generation_operation.adapter_decoder_version).into(),
                    command.pin.local_counter_id.clone().into(),
                    i64::from(command.pin.local_counter_version).into(),
                    i64::from(command.pin.fallback_policy_version).into(),
                    i64::try_from(command.pin.safety_margin_tokens).map_err(|_| StorageError::InvalidArgument("count safety margin is too large".into()))?.into(),
                    pin_digest.clone().into(),
                    candidate_ref.clone().into(),
                    candidate_digest.into(),
                    request_digest.into(),
                    request_ref.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        transaction
            .execute_raw(sql(
                "INSERT INTO effects (id, node_instance_id, count_call_id, effect_kind, classification, operation_key, idempotency_key, retry_policy_json, status, created_at) VALUES (?, ?, ?, 'token_count', ?, 'llm.count_tokens', ?, ?, 'pending', ?)",
                vec![
                    command.effect_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    command.count_call_id.clone().into(),
                    classification_name(EffectClassification::Pure).into(),
                    command.effect_idempotency_key.clone().into(),
                    retry_json.into(),
                    now.into(),
                ],
            ))
            .await?;
        transaction
            .execute_raw(sql(
                "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, 1, 'prepared', ?)",
                vec![
                    command.effect_attempt_id.clone().into(),
                    command.effect_id.clone().into(),
                    command.originating_attempt_id.clone().into(),
                    request_ref.clone().into(),
                ],
            ))
            .await?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        add_count_refs(
            &transaction,
            &command.count_call_id,
            &command.effect_attempt_id,
            &candidate_ref,
            &request_ref,
            now,
        )
        .await?;
        append_count_event(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
            "llm.count.prepared",
            json!({
                "schemaVersion":1,
                "countCallId":command.count_call_id,
                "effectId":command.effect_id,
                "effectAttemptId":command.effect_attempt_id,
                "countOrdinal":command.count_ordinal,
                "countExecutionPinDigest":pin_digest,
                "trimCandidateRef":candidate_ref,
                "requestRef":request_ref,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(PreparedCountCall {
            count_call_id: command.count_call_id,
            effect_id: command.effect_id,
            effect_attempt_id: command.effect_attempt_id,
            trim_candidate_ref: candidate_ref,
            request_ref,
            logical_status: LlmLogicalCallStatus::Prepared,
            effect_status: EffectStatus::Pending,
            attempt_status: EffectAttemptStatus::Prepared,
            replayed: false,
        })
    }

    pub async fn start_count_call(
        &self,
        command: StartCountCallCommand,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let call =
            load_count_attempt(&transaction, &command.effect_attempt_id, &command.fence).await?;
        validate_count_fence(&call, &command.fence)?;
        if !call.provider_count_available {
            return Err(StorageError::InvalidArgument(
                "provider count operation is not pinned".into(),
            ));
        }
        let context = load_ledger_context(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        validate_count_checkpoint(
            &command.checkpoint,
            expectation(
                &context,
                &call,
                &command.effect_attempt_id,
                &command.fence.invoking_node_attempt_id,
                LlmLogicalCallStatus::Running,
                None,
                None,
            ),
        )?;
        if call.attempt_status == "started"
            && call.effect_status == "pending"
            && call.count_status == "running"
        {
            if call.attempt_provider_request_id == command.provider_request_id
                && call.checkpoint_digest.as_deref() == Some(&command.checkpoint.checksum)
            {
                transaction.commit().await?;
                return Ok(());
            }
            return Err(StorageError::Conflict("count_call_start_replay"));
        }
        if call.attempt_status != "prepared"
            || call.effect_status != "pending"
            || call.count_status != "prepared"
        {
            return Err(StorageError::Conflict("count_effect_status"));
        }
        let attempt = transaction.execute_raw(sql(
            "UPDATE effect_attempts SET status = 'started', provider_request_id = ?, started_at = ? WHERE id = ? AND status = 'prepared'",
            vec![command.provider_request_id.into(), now.into(), command.effect_attempt_id.clone().into()],
        )).await?;
        let count = transaction
            .execute_raw(sql(
                "UPDATE count_calls SET status = 'running' WHERE id = ? AND status = 'prepared'",
                vec![call.count_call_id.clone().into()],
            ))
            .await?;
        if attempt.rows_affected() != 1 || count.rows_affected() != 1 {
            return Err(StorageError::Conflict("count_effect_status"));
        }
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        append_count_event(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
            "llm.count.started",
            json!({
                "schemaVersion":1,
                "countCallId":call.count_call_id,
                "effectId":call.effect_id,
                "effectAttemptId":command.effect_attempt_id,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn prepare_count_call_retry(
        &self,
        command: PrepareCountCallRetryCommand,
        now: i64,
    ) -> StorageResult<PreparedCountCall> {
        prepare_retry(self, command, now).await
    }
}

fn validate_prepare(command: &PrepareCountCallCommand) -> StorageResult<()> {
    if [
        &command.count_call_id,
        &command.effect_id,
        &command.effect_attempt_id,
        &command.node_instance_id,
        &command.originating_attempt_id,
        &command.channel_id,
        &command.effect_idempotency_key,
    ]
    .iter()
    .any(|value| value.is_empty() || value.len() > 256)
        || command.count_ordinal == 0
        || command.trim_candidate_bytes.is_empty()
        || command.trim_candidate_bytes.len() > 16 * 1024 * 1024
        || command.request_bytes.is_empty()
        || command.request_bytes.len() > 16 * 1024 * 1024
        || command.retry_policy.max_attempts == 0
        || command.retry_policy.max_attempts > 32
    {
        return Err(StorageError::InvalidArgument(
            "count prepare command is outside supported bounds".into(),
        ));
    }
    Ok(())
}

fn expectation<'a>(
    context: &'a super::validation::LedgerContext,
    call: &'a super::count_validation::FencedCountCall,
    effect_attempt_id: &'a str,
    updater_attempt_id: &'a str,
    status: LlmLogicalCallStatus,
    result_source: Option<zhuangsheng_core::llm::CountResultSource>,
    result_ref: Option<&'a str>,
) -> CountCheckpointExpectation<'a> {
    CountCheckpointExpectation {
        context,
        node_instance_id: &call.node_instance_id,
        updater_attempt_id,
        count_call_id: &call.count_call_id,
        effect_id: &call.effect_id,
        effect_attempt_id,
        count_ordinal: call.count_ordinal,
        pin_digest: &call.pin_digest,
        candidate_ref: &call.candidate_ref,
        candidate_digest: &call.candidate_digest,
        request_digest: &call.request_digest,
        status,
        result_source,
        result_ref,
    }
}
