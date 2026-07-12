use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{LlmLogicalCallStatus, LlmLoopCheckpoint, ToolCallCheckpointStatus};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

use super::model_ledger_helpers::persist_checkpoint;

pub(crate) async fn supersede_prepared_effect_attempts<C: ConnectionTrait>(
    connection: &C,
    invoking_attempt_id: &str,
    now: i64,
) -> StorageResult<u64> {
    let rows = connection
        .query_all(sql(
            "SELECT ea.id AS effect_attempt_id, e.id AS effect_id, e.node_instance_id, e.model_call_id, e.count_call_id, e.tool_call_id FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id WHERE ea.invoking_node_attempt_id = ? AND ea.status = 'prepared' ORDER BY ea.id",
            vec![invoking_attempt_id.into()],
        ))
        .await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let node_instance_id = rows[0].try_get::<String>("", "node_instance_id")?;
    let checkpoint_row = connection
        .query_one(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("LLM checkpoint missing during recovery".into()))?;
    let checkpoint_object_id: String = checkpoint_row.try_get("", "checkpoint_object_id")?;
    let mut checkpoint: LlmLoopCheckpoint =
        load_object_json(connection, &checkpoint_object_id).await?;
    if !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != node_instance_id
        || checkpoint.last_updated_by_attempt_id != invoking_attempt_id
    {
        return Err(StorageError::Integrity(
            "LLM checkpoint is incompatible with prepared effect recovery".into(),
        ));
    }
    for row in &rows {
        if row.try_get::<String>("", "node_instance_id")? != node_instance_id {
            return Err(StorageError::Integrity(
                "invoking attempt owns effects from multiple node instances".into(),
            ));
        }
        let effect_attempt_id: String = row.try_get("", "effect_attempt_id")?;
        let effect_id: String = row.try_get("", "effect_id")?;
        let updated = connection
            .execute(sql(
                "UPDATE effect_attempts SET status = 'superseded_before_start', finished_at = ? WHERE id = ? AND invoking_node_attempt_id = ? AND status = 'prepared'",
                vec![now.into(), effect_attempt_id.clone().into(), invoking_attempt_id.into()],
            ))
            .await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("effect_attempt_recovery"));
        }
        let model_call_id: Option<String> = row.try_get("", "model_call_id")?;
        let count_call_id: Option<String> = row.try_get("", "count_call_id")?;
        let tool_call_id: Option<String> = row.try_get("", "tool_call_id")?;
        match (model_call_id, count_call_id, tool_call_id) {
            (Some(owner_id), None, None) => {
                update_owner(connection, "model_calls", &owner_id).await?;
                let active = checkpoint.active_model_effect.as_mut().ok_or_else(|| {
                    StorageError::Integrity("active model effect missing during recovery".into())
                })?;
                if active.model_call_id != owner_id
                    || active.effect_id != effect_id
                    || active.status != LlmLogicalCallStatus::Prepared
                {
                    return Err(StorageError::Integrity(
                        "active model effect does not match prepared ledger row".into(),
                    ));
                }
                active.status = LlmLogicalCallStatus::RetryReady;
            }
            (None, Some(owner_id), None) => {
                update_owner(connection, "count_calls", &owner_id).await?;
                let active = checkpoint.active_count_effect.as_mut().ok_or_else(|| {
                    StorageError::Integrity("active count effect missing during recovery".into())
                })?;
                if active.count_call_id != owner_id
                    || active.effect_id != effect_id
                    || active.status != LlmLogicalCallStatus::Prepared
                {
                    return Err(StorageError::Integrity(
                        "active count effect does not match prepared ledger row".into(),
                    ));
                }
                active.status = LlmLogicalCallStatus::RetryReady;
            }
            (None, None, Some(owner_id)) => {
                update_owner(connection, "tool_calls", &owner_id).await?;
                let active = checkpoint
                    .current_batch
                    .iter_mut()
                    .find(|call| {
                        call.tool_call_id == owner_id
                            && call.effect_id.as_deref() == Some(&effect_id)
                    })
                    .ok_or_else(|| {
                        StorageError::Integrity("tool effect missing from checkpoint batch".into())
                    })?;
                if active.status != ToolCallCheckpointStatus::Prepared {
                    return Err(StorageError::Integrity(
                        "tool checkpoint is not prepared during recovery".into(),
                    ));
                }
                active.status = ToolCallCheckpointStatus::RetryReady;
            }
            _ => {
                return Err(StorageError::Integrity(
                    "effect owner association is invalid during recovery".into(),
                ));
            }
        }
    }
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await?;
    u64::try_from(rows.len())
        .map_err(|_| StorageError::Integrity("prepared effect count overflow".into()))
}

async fn update_owner<C: ConnectionTrait>(
    connection: &C,
    table: &str,
    owner_id: &str,
) -> StorageResult<()> {
    let statement = match table {
        "model_calls" => {
            "UPDATE model_calls SET status = 'retry_ready' WHERE id = ? AND status = 'prepared'"
        }
        "count_calls" => {
            "UPDATE count_calls SET status = 'retry_ready' WHERE id = ? AND status = 'prepared'"
        }
        "tool_calls" => {
            "UPDATE tool_calls SET status = 'retry_ready' WHERE id = ? AND status = 'prepared'"
        }
        _ => return Err(StorageError::Integrity("unknown effect owner table".into())),
    };
    if connection
        .execute(sql(statement, vec![owner_id.into()]))
        .await?
        .rows_affected()
        != 1
    {
        return Err(StorageError::Conflict("effect_owner_recovery"));
    }
    Ok(())
}
