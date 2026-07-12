use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectAttemptStatus, EffectStatus, PrepareToolCallCommand, PreparedToolCall,
        StartToolCallCommand, ToolCallCheckpointStatus,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::{
    model_ledger_helpers::{classification_name, persist_checkpoint},
    tool_ledger_helpers::{add_prepare_refs, append_tool_event, load_existing},
    tool_ledger_retry::prepare_retry,
    tool_validation::{
        ToolCheckpointExpectation, load_tool_attempt, validate_tool_checkpoint,
        validate_tool_fence, validate_tool_material, validate_tool_start_policy,
    },
    validation::load_ledger_context,
};

impl SqliteStore {
    pub async fn prepare_tool_call(
        &self,
        command: PrepareToolCallCommand,
        now: i64,
    ) -> StorageResult<PreparedToolCall> {
        validate_prepare_fields(&command)?;
        let transaction = self.db.begin().await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        let material = validate_tool_material(&context, &command)?;
        if material.requires_approval {
            return Err(StorageError::InvalidArgument(
                "tool call requires an approval batch".into(),
            ));
        }
        let retry_json = canonical::to_string(&command.retry_policy)?;
        if let Some(existing) = load_existing(&transaction, &command, &retry_json).await? {
            transaction.commit().await?;
            return Ok(existing);
        }
        validate_model_owner(&transaction, &command).await?;
        let model_count: i64 = transaction
            .query_one(sql(
                "SELECT COUNT(*) AS count FROM tool_calls WHERE model_call_id = ?",
                vec![command.model_call_id.clone().into()],
            ))
            .await?
            .expect("tool count query returns a row")
            .try_get("", "count")?;
        if u64::try_from(model_count).ok() != Some(command.call_index) {
            return Err(StorageError::InvalidArgument(
                "tool call index is not the next stable model-call index".into(),
            ));
        }
        let total: i64 = transaction
            .query_one(sql(
                "SELECT COUNT(*) AS count FROM tool_calls WHERE node_instance_id = ?",
                vec![command.node_instance_id.clone().into()],
            ))
            .await?
            .expect("tool budget query returns a row")
            .try_get("", "count")?;
        let expected_used = u64::try_from(total)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| StorageError::Integrity("invalid tool-call count".into()))?;
        let limit = context
            .snapshot
            .limits
            .max_tool_calls
            .ok_or_else(|| StorageError::Integrity("tool-call limit is not pinned".into()))?;
        if expected_used > limit {
            return Err(StorageError::InvalidArgument(
                "tool-call limit exceeded".into(),
            ));
        }
        validate_tool_checkpoint(
            &command.checkpoint,
            expectation(
                &context,
                &command.node_instance_id,
                &command.originating_attempt_id,
                &command.model_call_id,
                &command.tool_call_id,
                &command.effect_id,
                &command.effect_attempt_id,
                command.call_index,
                &command.call_digest,
                expected_used,
                ToolCallCheckpointStatus::Prepared,
                None,
            ),
        )?;
        let arguments_ref =
            put_inline_object(&transaction, &canonical::to_vec(&material.arguments)?, now).await?;
        transaction
            .execute(sql(
                "INSERT INTO tool_calls (id, node_instance_id, originating_attempt_id, model_call_id, provider_call_id, call_index, binding_id, tool_id, tool_version, call_digest, arguments_object_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'prepared', ?)",
                vec![
                    command.tool_call_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    command.originating_attempt_id.clone().into(),
                    command.model_call_id.clone().into(),
                    command.provider_call_id.clone().into(),
                    i64::try_from(command.call_index).map_err(|_| StorageError::InvalidArgument("tool call index is too large".into()))?.into(),
                    command.binding_id.clone().into(),
                    command.tool_id.clone().into(),
                    command.tool_version.clone().into(),
                    command.call_digest.clone().into(),
                    arguments_ref.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        transaction
            .execute(sql(
                "INSERT INTO effects (id, node_instance_id, tool_call_id, effect_kind, classification, operation_key, idempotency_key, retry_policy_json, status, created_at) VALUES (?, ?, ?, 'custom_tool', ?, ?, ?, ?, 'pending', ?)",
                vec![
                    command.effect_id.clone().into(),
                    command.node_instance_id.clone().into(),
                    command.tool_call_id.clone().into(),
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
                    arguments_ref.clone().into(),
                ],
            ))
            .await?;
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        add_prepare_refs(
            &transaction,
            &command.tool_call_id,
            &command.effect_attempt_id,
            &arguments_ref,
            now,
        )
        .await?;
        append_tool_event(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
            "llm.tool.prepared",
            json!({
                "schemaVersion":1,
                "toolCallId":command.tool_call_id,
                "modelCallId":command.model_call_id,
                "callIndex":command.call_index,
                "callDigest":command.call_digest,
                "effectId":command.effect_id,
                "effectAttemptId":command.effect_attempt_id,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(PreparedToolCall {
            tool_call_id: command.tool_call_id,
            effect_id: Some(command.effect_id),
            effect_attempt_id: Some(command.effect_attempt_id),
            arguments_ref,
            status: ToolCallCheckpointStatus::Prepared,
            effect_status: Some(EffectStatus::Pending),
            attempt_status: Some(EffectAttemptStatus::Prepared),
            replayed: false,
        })
    }

    pub async fn start_tool_call(
        &self,
        command: StartToolCallCommand,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let call =
            load_tool_attempt(&transaction, &command.effect_attempt_id, &command.fence).await?;
        validate_tool_fence(&call, &command.fence)?;
        let context = load_ledger_context(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        validate_tool_checkpoint(
            &command.checkpoint,
            expectation(
                &context,
                &call.node_instance_id,
                &command.fence.invoking_node_attempt_id,
                &call.model_call_id,
                &call.tool_call_id,
                &call.effect_id,
                &command.effect_attempt_id,
                call.call_index,
                &call.call_digest,
                call.tool_calls_used,
                ToolCallCheckpointStatus::Running,
                None,
            ),
        )?;
        if call.attempt_status == "started"
            && call.effect_status == "pending"
            && call.tool_status == "running"
        {
            if call.attempt_provider_request_id == command.provider_request_id
                && call.checkpoint_digest.as_deref() == Some(&command.checkpoint.checksum)
            {
                transaction.commit().await?;
                return Ok(());
            }
            return Err(StorageError::Conflict("tool_call_start_replay"));
        }
        if call.attempt_status != "prepared"
            || call.effect_status != "pending"
            || call.tool_status != "prepared"
        {
            return Err(StorageError::Conflict("tool_effect_status"));
        }
        validate_tool_start_policy(&transaction, &context, &call).await?;
        let attempt = transaction.execute(sql(
            "UPDATE effect_attempts SET status = 'started', provider_request_id = ?, started_at = ? WHERE id = ? AND status = 'prepared'",
            vec![command.provider_request_id.into(), now.into(), command.effect_attempt_id.clone().into()],
        )).await?;
        let tool = transaction
            .execute(sql(
                "UPDATE tool_calls SET status = 'running' WHERE id = ? AND status = 'prepared'",
                vec![call.tool_call_id.clone().into()],
            ))
            .await?;
        if attempt.rows_affected() != 1 || tool.rows_affected() != 1 {
            return Err(StorageError::Conflict("tool_effect_status"));
        }
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        append_tool_event(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
            "llm.tool.started",
            json!({
                "schemaVersion":1,
                "toolCallId":call.tool_call_id,
                "effectId":call.effect_id,
                "effectAttemptId":command.effect_attempt_id,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn prepare_tool_call_retry(
        &self,
        command: zhuangsheng_core::llm::PrepareToolCallRetryCommand,
        now: i64,
    ) -> StorageResult<PreparedToolCall> {
        prepare_retry(self, command, now).await
    }
}

async fn validate_model_owner<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolCallCommand,
) -> StorageResult<()> {
    let row = connection
        .query_one(sql(
            "SELECT node_instance_id, status FROM model_calls WHERE id = ?",
            vec![command.model_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "model_call",
            id: command.model_call_id.clone(),
        })?;
    if row.try_get::<String>("", "node_instance_id")? != command.node_instance_id
        || row.try_get::<String>("", "status")? != "completed"
    {
        return Err(StorageError::InvalidArgument(
            "tool call model owner is incompatible".into(),
        ));
    }
    Ok(())
}

fn validate_prepare_fields(command: &PrepareToolCallCommand) -> StorageResult<()> {
    if [
        &command.tool_call_id,
        &command.effect_id,
        &command.effect_attempt_id,
        &command.node_instance_id,
        &command.originating_attempt_id,
        &command.model_call_id,
        &command.binding_id,
        &command.tool_id,
        &command.tool_version,
        &command.call_digest,
        &command.descriptor_digest,
        &command.implementation_digest,
        &command.effect_operation_key,
        &command.effect_idempotency_key,
    ]
    .iter()
    .any(|value| value.is_empty() || value.len() > 256)
        || command.arguments_bytes.is_empty()
        || command.arguments_bytes.len() > 1024 * 1024
        || command.retry_policy.max_attempts == 0
        || command.retry_policy.max_attempts > 32
    {
        return Err(StorageError::InvalidArgument(
            "tool prepare command is outside supported bounds".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn expectation<'a>(
    context: &'a super::validation::LedgerContext,
    node_instance_id: &'a str,
    updater_attempt_id: &'a str,
    model_call_id: &'a str,
    tool_call_id: &'a str,
    effect_id: &'a str,
    effect_attempt_id: &'a str,
    call_index: u64,
    call_digest: &'a str,
    tool_calls_used: u64,
    status: ToolCallCheckpointStatus,
    output_ref: Option<&'a str>,
) -> ToolCheckpointExpectation<'a> {
    ToolCheckpointExpectation {
        context,
        node_instance_id,
        updater_attempt_id,
        model_call_id,
        tool_call_id,
        effect_id,
        effect_attempt_id,
        call_index,
        call_digest,
        expected_tool_calls_used: tool_calls_used,
        status,
        output_ref,
    }
}
