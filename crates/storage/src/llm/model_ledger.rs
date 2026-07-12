use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectAttemptStatus, EffectStatus, LlmLogicalCallStatus, PrepareModelCallCommand,
        PrepareModelCallRetryCommand, PreparedModelCall, StartModelCallCommand,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::model_ledger_helpers::{
    add_ref, classification_name, load_existing, persist_checkpoint, validate_prepare_fields,
};
use super::model_ledger_replay::{ReplayDecision, classify_start, load_retry_replay};
use super::validation::{
    CheckpointExpectation, load_ledger_context, validate_checkpoint, validate_fence,
    validate_node_attempt_fence, validate_operation,
};

impl SqliteStore {
    pub async fn prepare_model_call(
        &self,
        command: PrepareModelCallCommand,
        now: i64,
    ) -> StorageResult<PreparedModelCall> {
        validate_prepare_fields(&command)?;
        let transaction = self.db.begin().await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        validate_operation(
            &context.snapshot.operation,
            &command.operation,
            &command.channel_id,
            &context,
        )?;
        validate_checkpoint(
            &command.checkpoint,
            CheckpointExpectation {
                context: &context,
                node_instance_id: &command.node_instance_id,
                updater_attempt_id: &command.originating_attempt_id,
                call_no: command.call_no,
                model_call_id: &command.model_call_id,
                effect_id: &command.effect_id,
                effect_attempt_id: &command.effect_attempt_id,
                status: LlmLogicalCallStatus::Prepared,
                response_ref: None,
            },
        )?;
        let operation_json = canonical::to_string(&command.operation.operation_key)?;
        let request_digest = canonical::hash_bytes(&command.request_bytes);
        let retry_json = canonical::to_string(&command.retry_policy)?;
        if let Some(existing) = load_existing(
            &transaction,
            &command,
            &operation_json,
            &request_digest,
            &retry_json,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(existing);
        }
        let count: i64 = transaction
            .query_one(sql(
                "SELECT COUNT(*) AS count FROM model_calls WHERE node_instance_id = ?",
                vec![command.node_instance_id.clone().into()],
            ))
            .await?
            .expect("count query returns a row")
            .try_get("", "count")?;
        let expected_call_no = u64::try_from(count)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| StorageError::Integrity("invalid model call count".into()))?;
        let limit = context
            .snapshot
            .limits
            .max_model_calls
            .ok_or_else(|| StorageError::Integrity("model call limit is not pinned".into()))?;
        if command.call_no != expected_call_no || command.call_no > limit {
            return Err(StorageError::InvalidArgument(
                "model call number is non-sequential or exceeds its limit".into(),
            ));
        }
        let request_object_id =
            put_inline_object(&transaction, &command.request_bytes, now).await?;
        transaction
            .execute(sql(
                "INSERT INTO model_calls (id, node_instance_id, originating_attempt_id, call_no, channel_id, channel_revision_id, model_id, operation_key_json, operation_taxonomy_version, adapter_decoder_version, request_object_id, status, started_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'prepared', ?)",
                vec![
                    command.model_call_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    command.originating_attempt_id.clone().into(),
                    i64::try_from(command.call_no).map_err(|_| StorageError::InvalidArgument("model call number is too large".into()))?.into(),
                    command.channel_id.clone().into(),
                    command.operation.channel_revision_id.clone().into(),
                    command.operation.model_id.clone().into(),
                    operation_json.into(),
                    i64::from(command.operation.operation_taxonomy_version).into(),
                    i64::from(command.operation.adapter_decoder_version).into(),
                    request_object_id.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        transaction
            .execute(sql(
                "INSERT INTO effects (id, node_instance_id, model_call_id, effect_kind, classification, operation_key, idempotency_key, retry_policy_json, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
                vec![
                    command.effect_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    command.model_call_id.clone().into(),
                    command.effect_kind.clone().into(),
                    classification_name(command.effect_classification).into(),
                    command.effect_operation_key.clone().into(),
                    command.effect_idempotency_key.clone().into(),
                    retry_json.into(),
                    now.into(),
                ],
            ))
            .await?;
        transaction
            .execute(sql(
                "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, 1, 'prepared', ?)",
                vec![
                    command.effect_attempt_id.clone().into(),
                    command.effect_id.clone().into(),
                    command.originating_attempt_id.clone().into(),
                    request_object_id.clone().into(),
                ],
            ))
            .await?;
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        add_ref(
            &transaction,
            &request_object_id,
            "model_call",
            &command.model_call_id,
            "request",
            now,
        )
        .await?;
        add_ref(
            &transaction,
            &request_object_id,
            "effect_attempt",
            &command.effect_attempt_id,
            "request",
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(PreparedModelCall {
            model_call_id: command.model_call_id,
            effect_id: command.effect_id,
            effect_attempt_id: command.effect_attempt_id,
            model_status: LlmLogicalCallStatus::Prepared,
            effect_status: EffectStatus::Pending,
            attempt_status: EffectAttemptStatus::Prepared,
            replayed: false,
        })
    }

    pub async fn start_model_call(
        &self,
        command: StartModelCallCommand,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let fenced =
            validate_fence(&transaction, &command.effect_attempt_id, &command.fence).await?;
        let context = load_ledger_context(
            &transaction,
            &fenced.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        validate_checkpoint(
            &command.checkpoint,
            CheckpointExpectation {
                context: &context,
                node_instance_id: &fenced.node_instance_id,
                updater_attempt_id: &command.fence.invoking_node_attempt_id,
                call_no: fenced.call_no,
                model_call_id: &fenced.model_call_id,
                effect_id: &fenced.effect_id,
                effect_attempt_id: &command.effect_attempt_id,
                status: LlmLogicalCallStatus::Running,
                response_ref: None,
            },
        )?;
        match classify_start(
            &fenced,
            &command.provider_request_id,
            &command.checkpoint.checksum,
        )? {
            ReplayDecision::Replayed => {
                transaction.commit().await?;
                return Ok(());
            }
            ReplayDecision::Fresh => {}
        }
        let updated = transaction
            .execute(sql(
                "UPDATE effect_attempts SET status = 'started', provider_request_id = ?, started_at = ? WHERE id = ? AND status = 'prepared' AND invoking_node_attempt_id = ?",
                vec![command.provider_request_id.clone().into(), now.into(), command.effect_attempt_id.clone().into(), command.fence.invoking_node_attempt_id.clone().into()],
            ))
            .await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("effect_attempt_status"));
        }
        let model = transaction
            .execute(sql(
                "UPDATE model_calls SET status = 'running', provider_request_id = ? WHERE id = ? AND status = 'prepared'",
                vec![command.provider_request_id.into(), fenced.model_call_id.into()],
            ))
            .await?;
        if model.rows_affected() != 1 {
            return Err(StorageError::Conflict("model_call_status"));
        }
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn prepare_model_call_retry(
        &self,
        command: PrepareModelCallRetryCommand,
        now: i64,
    ) -> StorageResult<PreparedModelCall> {
        if command.model_call_id.is_empty() || command.effect_attempt_id.is_empty() {
            return Err(StorageError::InvalidArgument(
                "model call retry ids are required".into(),
            ));
        }
        let transaction = self.db.begin().await?;
        if let Some(replay) = load_retry_replay(&transaction, &command).await? {
            validate_node_attempt_fence(&transaction, &replay.node_instance_id, &command.fence)
                .await?;
            let context = load_ledger_context(
                &transaction,
                &replay.node_instance_id,
                &command.fence.invoking_node_attempt_id,
            )
            .await?;
            validate_checkpoint(
                &command.checkpoint,
                CheckpointExpectation {
                    context: &context,
                    node_instance_id: &replay.node_instance_id,
                    updater_attempt_id: &command.fence.invoking_node_attempt_id,
                    call_no: replay.call_no,
                    model_call_id: &command.model_call_id,
                    effect_id: &replay.effect_id,
                    effect_attempt_id: &command.effect_attempt_id,
                    status: LlmLogicalCallStatus::Prepared,
                    response_ref: None,
                },
            )?;
            transaction.commit().await?;
            return Ok(PreparedModelCall {
                model_call_id: command.model_call_id,
                effect_id: replay.effect_id,
                effect_attempt_id: command.effect_attempt_id,
                model_status: LlmLogicalCallStatus::Prepared,
                effect_status: EffectStatus::Pending,
                attempt_status: EffectAttemptStatus::Prepared,
                replayed: true,
            });
        }
        let row = transaction
            .query_one(sql(
                "SELECT mc.node_instance_id, mc.call_no, mc.request_object_id, mc.status AS model_status, e.id AS effect_id, e.status AS effect_status, e.classification, e.retry_policy_json, COALESCE(MAX(ea.attempt_no), 0) AS attempt_count FROM model_calls mc JOIN effects e ON e.model_call_id = mc.id LEFT JOIN effect_attempts ea ON ea.effect_id = e.id WHERE mc.id = ? GROUP BY mc.id, e.id",
                vec![command.model_call_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::NotFound {
                kind: "model_call",
                id: command.model_call_id.clone(),
            })?;
        let node_instance_id: String = row.try_get("", "node_instance_id")?;
        let model_status: String = row.try_get("", "model_status")?;
        let effect_status: String = row.try_get("", "effect_status")?;
        let classification: String = row.try_get("", "classification")?;
        if model_status != "retry_ready"
            || effect_status != "pending"
            || classification == "non_idempotent"
        {
            return Err(StorageError::Conflict("model_effect_retry_status"));
        }
        validate_node_attempt_fence(&transaction, &node_instance_id, &command.fence).await?;
        let retry_policy: zhuangsheng_core::llm::EffectRetryPolicy =
            serde_json::from_str(&row.try_get::<String>("", "retry_policy_json")?)
                .map_err(|error| StorageError::Integrity(error.to_string()))?;
        let attempt_count: i64 = row.try_get("", "attempt_count")?;
        let next_attempt = attempt_count
            .checked_add(1)
            .ok_or_else(|| StorageError::Integrity("effect attempt count overflow".into()))?;
        if next_attempt > i64::from(retry_policy.max_attempts) {
            return Err(StorageError::InvalidArgument(
                "model effect retry limit exceeded".into(),
            ));
        }
        let call_no = u64::try_from(row.try_get::<i64>("", "call_no")?)
            .map_err(|_| StorageError::Integrity("invalid model call number".into()))?;
        let effect_id: String = row.try_get("", "effect_id")?;
        let context = load_ledger_context(
            &transaction,
            &node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        validate_checkpoint(
            &command.checkpoint,
            CheckpointExpectation {
                context: &context,
                node_instance_id: &node_instance_id,
                updater_attempt_id: &command.fence.invoking_node_attempt_id,
                call_no,
                model_call_id: &command.model_call_id,
                effect_id: &effect_id,
                effect_attempt_id: &command.effect_attempt_id,
                status: LlmLogicalCallStatus::Prepared,
                response_ref: None,
            },
        )?;
        let request_object_id: String = row.try_get("", "request_object_id")?;
        transaction
            .execute(sql(
                "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, ?, 'prepared', ?)",
                vec![command.effect_attempt_id.clone().into(), effect_id.clone().into(), command.fence.invoking_node_attempt_id.clone().into(), next_attempt.into(), request_object_id.clone().into()],
            ))
            .await?;
        let updated = transaction
            .execute(sql(
                "UPDATE model_calls SET status = 'prepared' WHERE id = ? AND status = 'retry_ready'",
                vec![command.model_call_id.clone().into()],
            ))
            .await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("model_call_retry_status"));
        }
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        add_ref(
            &transaction,
            &request_object_id,
            "effect_attempt",
            &command.effect_attempt_id,
            "request",
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(PreparedModelCall {
            model_call_id: command.model_call_id,
            effect_id,
            effect_attempt_id: command.effect_attempt_id,
            model_status: LlmLogicalCallStatus::Prepared,
            effect_status: EffectStatus::Pending,
            attempt_status: EffectAttemptStatus::Prepared,
            replayed: false,
        })
    }
}
