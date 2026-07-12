use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::llm::{
    EffectAttemptStatus, EffectRetryPolicy, EffectStatus, PrepareToolCallRetryCommand,
    PreparedToolCall, ToolCallCheckpointStatus,
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::{
    model_ledger_helpers::{add_ref, persist_checkpoint},
    tool_ledger_helpers::append_tool_event,
    tool_validation::{ToolCheckpointExpectation, validate_tool_checkpoint},
    validation::{load_ledger_context, validate_node_attempt_fence},
};

pub(super) async fn prepare_retry(
    store: &SqliteStore,
    command: PrepareToolCallRetryCommand,
    now: i64,
) -> StorageResult<PreparedToolCall> {
    if command.tool_call_id.is_empty() || command.effect_attempt_id.is_empty() {
        return Err(StorageError::InvalidArgument(
            "tool retry ids are required".into(),
        ));
    }
    let transaction = store.db.begin().await?;
    if let Some(replay) = load_retry_replay(&transaction, &command).await? {
        validate_retry_checkpoint(&transaction, &command, &replay).await?;
        transaction.commit().await?;
        return Ok(result(&command, replay, true));
    }
    let row = transaction
        .query_one_raw(sql(
            "SELECT tc.node_instance_id, tc.model_call_id, tc.call_index, tc.call_digest, tc.arguments_object_id, tc.status AS tool_status, e.id AS effect_id, e.status AS effect_status, e.retry_policy_json, COALESCE(MAX(ea.attempt_no), 0) AS attempt_count, (SELECT COUNT(*) FROM tool_calls all_calls WHERE all_calls.node_instance_id = tc.node_instance_id) AS tool_calls_used FROM tool_calls tc JOIN effects e ON e.tool_call_id = tc.id LEFT JOIN effect_attempts ea ON ea.effect_id = e.id WHERE tc.id = ? GROUP BY tc.id, e.id",
            vec![command.tool_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "tool_call",
            id: command.tool_call_id.clone(),
        })?;
    if row.try_get::<String>("", "tool_status")? != "retry_ready"
        || row.try_get::<String>("", "effect_status")? != "pending"
    {
        return Err(StorageError::Conflict("tool_effect_retry_status"));
    }
    let retry = retry_context(&row)?;
    validate_retry_checkpoint(&transaction, &command, &retry).await?;
    let policy: EffectRetryPolicy =
        serde_json::from_str(&row.try_get::<String>("", "retry_policy_json")?)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let next_attempt = row
        .try_get::<i64>("", "attempt_count")?
        .checked_add(1)
        .ok_or_else(|| StorageError::Integrity("tool attempt number overflow".into()))?;
    if next_attempt > i64::from(policy.max_attempts) {
        return Err(StorageError::InvalidArgument(
            "tool retry limit exceeded".into(),
        ));
    }
    transaction
        .execute_raw(sql(
            "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, ?, 'prepared', ?)",
            vec![
                command.effect_attempt_id.clone().into(),
                retry.effect_id.clone().into(),
                command.fence.invoking_node_attempt_id.clone().into(),
                next_attempt.into(),
                retry.arguments_ref.clone().into(),
            ],
        ))
        .await?;
    if transaction
        .execute_raw(sql(
            "UPDATE tool_calls SET status = 'prepared' WHERE id = ? AND status = 'retry_ready'",
            vec![command.tool_call_id.clone().into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Err(StorageError::Conflict("tool_call_retry_status"));
    }
    persist_checkpoint(&transaction, &command.checkpoint, now).await?;
    add_ref(
        &transaction,
        &retry.arguments_ref,
        "effect_attempt",
        &command.effect_attempt_id,
        "request",
        now,
    )
    .await?;
    append_tool_event(
        &transaction,
        &retry.node_instance_id,
        &command.fence.invoking_node_attempt_id,
        "llm.tool.retry_prepared",
        json!({
            "schemaVersion":1,
            "toolCallId":command.tool_call_id,
            "effectId":retry.effect_id,
            "effectAttemptId":command.effect_attempt_id,
            "callIndex":retry.call_index,
        }),
        now,
    )
    .await?;
    transaction.commit().await?;
    Ok(result(&command, retry, false))
}

struct RetryContext {
    node_instance_id: String,
    model_call_id: String,
    effect_id: String,
    call_index: u64,
    call_digest: String,
    arguments_ref: String,
    tool_calls_used: u64,
}

async fn load_retry_replay<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolCallRetryCommand,
) -> StorageResult<Option<RetryContext>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ea.id AS effect_attempt_id, ea.invoking_node_attempt_id, ea.status AS attempt_status, ea.request_object_id AS attempt_request_object_id, e.id AS effect_id, e.status AS effect_status, tc.node_instance_id, tc.model_call_id, tc.call_index, tc.call_digest, tc.arguments_object_id, tc.status AS tool_status, cp.checkpoint_digest, (SELECT COUNT(*) FROM tool_calls all_calls WHERE all_calls.node_instance_id = tc.node_instance_id) AS tool_calls_used FROM effects e JOIN tool_calls tc ON tc.id = e.tool_call_id JOIN effect_attempts ea ON ea.effect_id = e.id LEFT JOIN llm_loop_checkpoints cp ON cp.node_instance_id = tc.node_instance_id WHERE tc.id = ? AND (ea.id = ? OR ea.invoking_node_attempt_id = ?) ORDER BY ea.attempt_no DESC LIMIT 1",
            vec![
                command.tool_call_id.clone().into(),
                command.effect_attempt_id.clone().into(),
                command.fence.invoking_node_attempt_id.clone().into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let matches = row.try_get::<String>("", "effect_attempt_id")? == command.effect_attempt_id
        && row.try_get::<String>("", "invoking_node_attempt_id")?
            == command.fence.invoking_node_attempt_id
        && row.try_get::<String>("", "attempt_status")? == "prepared"
        && row.try_get::<String>("", "effect_status")? == "pending"
        && row.try_get::<String>("", "tool_status")? == "prepared"
        && row.try_get::<String>("", "attempt_request_object_id")?
            == row.try_get::<String>("", "arguments_object_id")?
        && row
            .try_get::<Option<String>>("", "checkpoint_digest")?
            .as_deref()
            == Some(&command.checkpoint.checksum);
    if !matches {
        return Err(StorageError::Conflict("tool_call_retry_replay"));
    }
    Ok(Some(retry_context(&row)?))
}

fn retry_context(row: &sea_orm::QueryResult) -> StorageResult<RetryContext> {
    Ok(RetryContext {
        node_instance_id: row.try_get("", "node_instance_id")?,
        model_call_id: row.try_get("", "model_call_id")?,
        effect_id: row.try_get("", "effect_id")?,
        call_index: u64::try_from(row.try_get::<i64>("", "call_index")?)
            .map_err(|_| StorageError::Integrity("invalid tool call index".into()))?,
        call_digest: row.try_get("", "call_digest")?,
        arguments_ref: row.try_get("", "arguments_object_id")?,
        tool_calls_used: u64::try_from(row.try_get::<i64>("", "tool_calls_used")?)
            .map_err(|_| StorageError::Integrity("invalid tool-call count".into()))?,
    })
}

async fn validate_retry_checkpoint<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolCallRetryCommand,
    retry: &RetryContext,
) -> StorageResult<()> {
    validate_node_attempt_fence(connection, &retry.node_instance_id, &command.fence).await?;
    let context = load_ledger_context(
        connection,
        &retry.node_instance_id,
        &command.fence.invoking_node_attempt_id,
    )
    .await?;
    validate_tool_checkpoint(
        &command.checkpoint,
        ToolCheckpointExpectation {
            context: &context,
            node_instance_id: &retry.node_instance_id,
            updater_attempt_id: &command.fence.invoking_node_attempt_id,
            model_call_id: &retry.model_call_id,
            tool_call_id: &command.tool_call_id,
            effect_id: &retry.effect_id,
            effect_attempt_id: &command.effect_attempt_id,
            call_index: retry.call_index,
            call_digest: &retry.call_digest,
            expected_tool_calls_used: retry.tool_calls_used,
            status: ToolCallCheckpointStatus::Prepared,
            output_ref: None,
        },
    )
}

fn result(
    command: &PrepareToolCallRetryCommand,
    retry: RetryContext,
    replayed: bool,
) -> PreparedToolCall {
    PreparedToolCall {
        tool_call_id: command.tool_call_id.clone(),
        effect_id: Some(retry.effect_id),
        effect_attempt_id: Some(command.effect_attempt_id.clone()),
        arguments_ref: retry.arguments_ref,
        status: ToolCallCheckpointStatus::Prepared,
        effect_status: Some(EffectStatus::Pending),
        attempt_status: Some(EffectAttemptStatus::Prepared),
        replayed,
    }
}
