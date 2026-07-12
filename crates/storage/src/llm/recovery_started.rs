use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{LlmLogicalCallStatus, LlmLoopCheckpoint};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

use super::{
    effect_wait::{EffectWait, EffectWaitOwner, allocate_wait_id, open_effect_resolution_wait},
    model_ledger_helpers::{add_ref, persist_checkpoint},
};

pub(crate) enum StartedEffectRecovery {
    None,
    RetryReady(u64),
    Waiting { count: u64, wait_id: String },
}

pub(crate) async fn recover_started_effects<C: ConnectionTrait>(
    connection: &C,
    invoking_attempt_id: &str,
    error_object_id: &str,
    now: i64,
) -> StorageResult<StartedEffectRecovery> {
    let rows = connection
        .query_all(sql(
            "SELECT ea.id AS effect_attempt_id, e.id AS effect_id, e.node_instance_id, e.classification, e.model_call_id, e.count_call_id, e.tool_call_id FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id WHERE ea.invoking_node_attempt_id = ? AND ea.status = 'started' ORDER BY ea.id",
            vec![invoking_attempt_id.into()],
        ))
        .await?;
    if rows.is_empty() {
        return Ok(StartedEffectRecovery::None);
    }
    if rows.len() != 1 {
        return Err(StorageError::Integrity(
            "phase-one model recovery found multiple started effects".into(),
        ));
    }
    let row = &rows[0];
    let effect_attempt_id: String = row.try_get("", "effect_attempt_id")?;
    let effect_id: String = row.try_get("", "effect_id")?;
    let node_instance_id: String = row.try_get("", "node_instance_id")?;
    let model_call_id: Option<String> = row.try_get("", "model_call_id")?;
    let count_call_id: Option<String> = row.try_get("", "count_call_id")?;
    let tool_call_id: Option<String> = row.try_get("", "tool_call_id")?;
    let is_model = model_call_id.is_some() && count_call_id.is_none() && tool_call_id.is_none();
    let is_count = model_call_id.is_none() && count_call_id.is_some() && tool_call_id.is_none();
    let is_tool = model_call_id.is_none() && count_call_id.is_none() && tool_call_id.is_some();
    if !is_model && !is_count && !is_tool {
        return Err(StorageError::InvalidArgument(
            "started recovery owner is not supported".into(),
        ));
    }
    let classification: String = row.try_get("", "classification")?;
    let requires_coordination = (is_model || is_tool) && classification == "non_idempotent";
    let logical_status = if requires_coordination {
        LlmLogicalCallStatus::OutcomeUnknown
    } else {
        LlmLogicalCallStatus::RetryReady
    };
    let attempt = connection
        .execute(sql(
            "UPDATE effect_attempts SET status = 'outcome_unknown', error_object_id = ?, finished_at = ? WHERE id = ? AND status = 'started'",
            vec![
                error_object_id.into(),
                now.into(),
                effect_attempt_id.clone().into(),
            ],
        ))
        .await?;
    let owner = if let Some(model_call_id) = &model_call_id {
        connection
            .execute(sql(
                "UPDATE model_calls SET status = ? WHERE id = ? AND status = 'running'",
                vec![
                    if requires_coordination {
                        "outcome_unknown"
                    } else {
                        "retry_ready"
                    }
                    .into(),
                    model_call_id.clone().into(),
                ],
            ))
            .await?
    } else if let Some(count_call_id) = &count_call_id {
        connection
            .execute(sql(
                "UPDATE count_calls SET status = 'retry_ready' WHERE id = ? AND status = 'running'",
                vec![count_call_id.clone().into()],
            ))
            .await?
    } else {
        connection
            .execute(sql(
                "UPDATE tool_calls SET status = ? WHERE id = ? AND status = 'running'",
                vec![
                    if requires_coordination {
                        "outcome_unknown"
                    } else {
                        "retry_ready"
                    }
                    .into(),
                    tool_call_id.clone().expect("validated tool owner").into(),
                ],
            ))
            .await?
    };
    if attempt.rows_affected() != 1 || owner.rows_affected() != 1 {
        return Err(StorageError::Conflict("started_effect_recovery"));
    }
    if requires_coordination
        && connection
            .execute(sql(
                "UPDATE effects SET status = 'outcome_unknown', completed_at = ? WHERE id = ? AND status = 'pending'",
                vec![now.into(), effect_id.clone().into()],
            ))
            .await?
            .rows_affected()
            != 1
    {
        return Err(StorageError::Conflict("started_effect_recovery"));
    }
    let checkpoint_row = connection
        .query_one(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("started effect checkpoint missing".into()))?;
    let mut checkpoint: LlmLoopCheckpoint = load_object_json(
        connection,
        &checkpoint_row.try_get::<String>("", "checkpoint_object_id")?,
    )
    .await?;
    if !checkpoint.checksum_is_valid() {
        return Err(StorageError::Integrity(
            "started effect checkpoint checksum is invalid".into(),
        ));
    }
    if checkpoint.effect_watermark != effect_attempt_id {
        return Err(StorageError::Integrity(
            "started effect checkpoint watermark mismatch".into(),
        ));
    }
    if let Some(model_call_id) = &model_call_id {
        let active = checkpoint.active_model_effect.as_mut().ok_or_else(|| {
            StorageError::Integrity("started model effect checkpoint is missing".into())
        })?;
        if active.model_call_id != model_call_id.as_str()
            || active.effect_id != effect_id
            || active.status != LlmLogicalCallStatus::Running
        {
            return Err(StorageError::Integrity(
                "started model checkpoint projection mismatch".into(),
            ));
        }
        active.status = logical_status;
        active.response_ref = None;
    } else if let Some(count_call_id) = &count_call_id {
        let active = checkpoint.active_count_effect.as_mut().ok_or_else(|| {
            StorageError::Integrity("started count effect checkpoint is missing".into())
        })?;
        if active.count_call_id != count_call_id.as_str()
            || active.effect_id != effect_id
            || active.status != LlmLogicalCallStatus::Running
        {
            return Err(StorageError::Integrity(
                "started count checkpoint projection mismatch".into(),
            ));
        }
        active.status = LlmLogicalCallStatus::RetryReady;
        active.result_source = None;
        active.result_ref = None;
    } else {
        let tool_call_id = tool_call_id.as_ref().expect("validated tool owner");
        let active = checkpoint
            .current_batch
            .iter_mut()
            .find(|call| {
                call.tool_call_id == tool_call_id.as_str()
                    && call.effect_id.as_deref() == Some(effect_id.as_str())
            })
            .ok_or_else(|| {
                StorageError::Integrity("started tool effect checkpoint is missing".into())
            })?;
        if active.status != zhuangsheng_core::llm::ToolCallCheckpointStatus::Running {
            return Err(StorageError::Integrity(
                "started tool checkpoint projection mismatch".into(),
            ));
        }
        active.status = if requires_coordination {
            zhuangsheng_core::llm::ToolCallCheckpointStatus::OutcomeUnknown
        } else {
            zhuangsheng_core::llm::ToolCallCheckpointStatus::RetryReady
        };
        active.output_ref = None;
    }
    let wait_id = if requires_coordination {
        let wait_id = allocate_wait_id(connection, &node_instance_id).await?;
        checkpoint.wait_ids.push(wait_id.clone());
        Some(wait_id)
    } else {
        None
    };
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await?;
    add_ref(
        connection,
        error_object_id,
        "effect_attempt",
        &effect_attempt_id,
        "error",
        now,
    )
    .await?;
    if let Some(wait_id) = wait_id {
        let owner = if let Some(model_call_id) = &model_call_id {
            EffectWaitOwner::Model { model_call_id }
        } else {
            EffectWaitOwner::Tool {
                tool_call_id: tool_call_id.as_ref().expect("coordinated tool owner"),
            }
        };
        open_effect_resolution_wait(
            connection,
            EffectWait {
                wait_id: &wait_id,
                node_instance_id: &node_instance_id,
                invoking_node_attempt_id: invoking_attempt_id,
                owner,
                effect_id: &effect_id,
                effect_attempt_id: &effect_attempt_id,
                classification: &classification,
            },
            now,
        )
        .await?;
        Ok(StartedEffectRecovery::Waiting { count: 1, wait_id })
    } else {
        Ok(StartedEffectRecovery::RetryReady(1))
    }
}
