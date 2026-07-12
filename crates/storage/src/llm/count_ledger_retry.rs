use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::llm::{
    EffectAttemptStatus, EffectRetryPolicy, EffectStatus, LlmLogicalCallStatus,
    PrepareCountCallRetryCommand, PreparedCountCall,
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::{
    count_ledger_helpers::append_count_event,
    count_validation::{CountCheckpointExpectation, validate_count_checkpoint},
    model_ledger_helpers::{add_ref, persist_checkpoint},
    validation::{load_ledger_context, validate_node_attempt_fence},
};

pub(super) async fn prepare_retry(
    store: &SqliteStore,
    command: PrepareCountCallRetryCommand,
    now: i64,
) -> StorageResult<PreparedCountCall> {
    if command.count_call_id.is_empty() || command.effect_attempt_id.is_empty() {
        return Err(StorageError::InvalidArgument(
            "count retry ids are required".into(),
        ));
    }
    let transaction = store.db.begin().await?;
    if let Some(replayed) = load_retry_replay(&transaction, &command).await? {
        validate_retry_checkpoint(&transaction, &command, &replayed).await?;
        transaction.commit().await?;
        return Ok(PreparedCountCall {
            count_call_id: command.count_call_id,
            effect_id: replayed.effect_id,
            effect_attempt_id: command.effect_attempt_id,
            trim_candidate_ref: replayed.candidate_ref,
            request_ref: replayed.request_ref,
            context_snapshot_ref: command.checkpoint.context_snapshot_ref.clone(),
            transcript_ref: command.checkpoint.transcript_ref.clone(),
            logical_status: LlmLogicalCallStatus::Prepared,
            effect_status: EffectStatus::Pending,
            attempt_status: EffectAttemptStatus::Prepared,
            replayed: true,
        });
    }
    let row = transaction
        .query_one_raw(sql(
            "SELECT cc.node_instance_id, cc.count_ordinal, cc.count_execution_pin_digest, cc.trim_candidate_object_id, cc.trim_candidate_digest, cc.request_digest, cc.request_object_id, cc.status AS count_status, e.id AS effect_id, e.status AS effect_status, e.retry_policy_json, COALESCE(MAX(ea.attempt_no), 0) AS attempt_count FROM count_calls cc JOIN effects e ON e.count_call_id = cc.id LEFT JOIN effect_attempts ea ON ea.effect_id = e.id WHERE cc.id = ? GROUP BY cc.id, e.id",
            vec![command.count_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "count_call",
            id: command.count_call_id.clone(),
        })?;
    if row.try_get::<String>("", "count_status")? != "retry_ready"
        || row.try_get::<String>("", "effect_status")? != "pending"
    {
        return Err(StorageError::Conflict("count_effect_retry_status"));
    }
    let node_instance_id: String = row.try_get("", "node_instance_id")?;
    validate_node_attempt_fence(&transaction, &node_instance_id, &command.fence).await?;
    let policy: EffectRetryPolicy =
        serde_json::from_str(&row.try_get::<String>("", "retry_policy_json")?)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let next_attempt = row
        .try_get::<i64>("", "attempt_count")?
        .checked_add(1)
        .ok_or_else(|| StorageError::Integrity("count attempt number overflow".into()))?;
    if next_attempt > i64::from(policy.max_attempts) {
        return Err(StorageError::InvalidArgument(
            "count retry limit exceeded".into(),
        ));
    }
    let replay = retry_context(&row)?;
    validate_retry_checkpoint(&transaction, &command, &replay).await?;
    transaction
        .execute_raw(sql(
            "INSERT INTO effect_attempts (id, effect_id, invoking_node_attempt_id, attempt_no, status, request_object_id) VALUES (?, ?, ?, ?, 'prepared', ?)",
            vec![
                command.effect_attempt_id.clone().into(),
                replay.effect_id.clone().into(),
                command.fence.invoking_node_attempt_id.clone().into(),
                next_attempt.into(),
                replay.request_ref.clone().into(),
            ],
        ))
        .await?;
    if transaction
        .execute_raw(sql(
            "UPDATE count_calls SET status = 'prepared' WHERE id = ? AND status = 'retry_ready'",
            vec![command.count_call_id.clone().into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Err(StorageError::Conflict("count_call_retry_status"));
    }
    persist_checkpoint(&transaction, &command.checkpoint, now).await?;
    add_ref(
        &transaction,
        &replay.request_ref,
        "effect_attempt",
        &command.effect_attempt_id,
        "request",
        now,
    )
    .await?;
    append_count_event(
        &transaction,
        &replay.node_instance_id,
        &command.fence.invoking_node_attempt_id,
        "llm.count.retry_prepared",
        json!({
            "schemaVersion":1,
            "countCallId":command.count_call_id,
            "effectId":replay.effect_id,
            "effectAttemptId":command.effect_attempt_id,
            "countOrdinal":replay.count_ordinal,
        }),
        now,
    )
    .await?;
    transaction.commit().await?;
    Ok(PreparedCountCall {
        count_call_id: command.count_call_id,
        effect_id: replay.effect_id,
        effect_attempt_id: command.effect_attempt_id,
        trim_candidate_ref: replay.candidate_ref,
        request_ref: replay.request_ref,
        context_snapshot_ref: command.checkpoint.context_snapshot_ref,
        transcript_ref: command.checkpoint.transcript_ref,
        logical_status: LlmLogicalCallStatus::Prepared,
        effect_status: EffectStatus::Pending,
        attempt_status: EffectAttemptStatus::Prepared,
        replayed: false,
    })
}

struct RetryContext {
    node_instance_id: String,
    effect_id: String,
    count_ordinal: u64,
    pin_digest: String,
    candidate_ref: String,
    candidate_digest: String,
    request_digest: String,
    request_ref: String,
}

async fn load_retry_replay<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareCountCallRetryCommand,
) -> StorageResult<Option<RetryContext>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ea.id AS effect_attempt_id, ea.invoking_node_attempt_id, ea.status AS attempt_status, ea.request_object_id AS attempt_request_object_id, e.id AS effect_id, e.status AS effect_status, cc.node_instance_id, cc.count_ordinal, cc.count_execution_pin_digest, cc.trim_candidate_object_id, cc.trim_candidate_digest, cc.request_digest, cc.request_object_id, cc.status AS count_status, cp.checkpoint_digest FROM effects e JOIN count_calls cc ON cc.id = e.count_call_id JOIN effect_attempts ea ON ea.effect_id = e.id LEFT JOIN llm_loop_checkpoints cp ON cp.node_instance_id = cc.node_instance_id WHERE cc.id = ? AND (ea.id = ? OR ea.invoking_node_attempt_id = ?) ORDER BY ea.attempt_no DESC LIMIT 1",
            vec![
                command.count_call_id.clone().into(),
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
        && row.try_get::<String>("", "count_status")? == "prepared"
        && row.try_get::<String>("", "attempt_request_object_id")?
            == row.try_get::<String>("", "request_object_id")?
        && row
            .try_get::<Option<String>>("", "checkpoint_digest")?
            .as_deref()
            == Some(&command.checkpoint.checksum);
    if !matches {
        return Err(StorageError::Conflict("count_call_retry_replay"));
    }
    Ok(Some(retry_context(&row)?))
}

fn retry_context(row: &sea_orm::QueryResult) -> StorageResult<RetryContext> {
    Ok(RetryContext {
        node_instance_id: row.try_get("", "node_instance_id")?,
        effect_id: row.try_get("", "effect_id")?,
        count_ordinal: u64::try_from(row.try_get::<i64>("", "count_ordinal")?)
            .map_err(|_| StorageError::Integrity("invalid count ordinal".into()))?,
        pin_digest: row.try_get("", "count_execution_pin_digest")?,
        candidate_ref: row.try_get("", "trim_candidate_object_id")?,
        candidate_digest: row.try_get("", "trim_candidate_digest")?,
        request_digest: row.try_get("", "request_digest")?,
        request_ref: row.try_get("", "request_object_id")?,
    })
}

async fn validate_retry_checkpoint<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareCountCallRetryCommand,
    retry: &RetryContext,
) -> StorageResult<()> {
    validate_node_attempt_fence(connection, &retry.node_instance_id, &command.fence).await?;
    let context = load_ledger_context(
        connection,
        &retry.node_instance_id,
        &command.fence.invoking_node_attempt_id,
    )
    .await?;
    validate_count_checkpoint(
        &command.checkpoint,
        CountCheckpointExpectation {
            context: &context,
            node_instance_id: &retry.node_instance_id,
            updater_attempt_id: &command.fence.invoking_node_attempt_id,
            count_call_id: &command.count_call_id,
            effect_id: &retry.effect_id,
            effect_attempt_id: &command.effect_attempt_id,
            count_ordinal: retry.count_ordinal,
            pin_digest: &retry.pin_digest,
            candidate_ref: &retry.candidate_ref,
            candidate_digest: &retry.candidate_digest,
            request_digest: &retry.request_digest,
            status: LlmLogicalCallStatus::Prepared,
            result_source: None,
            result_ref: None,
        },
    )
}
