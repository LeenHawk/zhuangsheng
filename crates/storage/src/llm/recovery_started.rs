use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{LlmLogicalCallStatus, LlmLoopCheckpoint, ToolCallCheckpointStatus};

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

enum StartedOwner {
    Model(String),
    Count(String),
    Tool(String),
}

struct StartedEffect {
    effect_attempt_id: String,
    effect_id: String,
    node_instance_id: String,
    classification: String,
    owner: StartedOwner,
}

impl StartedEffect {
    fn requires_coordination(&self) -> bool {
        matches!(self.owner, StartedOwner::Model(_) | StartedOwner::Tool(_))
            && self.classification == "non_idempotent"
    }
}

pub(crate) async fn recover_started_effects<C: ConnectionTrait>(
    connection: &C,
    invoking_attempt_id: &str,
    error_object_id: &str,
    now: i64,
) -> StorageResult<StartedEffectRecovery> {
    let rows = connection.query_all(sql(
        "SELECT ea.id AS effect_attempt_id, e.id AS effect_id, e.node_instance_id, e.classification, e.model_call_id, e.count_call_id, e.tool_call_id, tc.call_index FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id LEFT JOIN tool_calls tc ON tc.id = e.tool_call_id WHERE ea.invoking_node_attempt_id = ? AND ea.status = 'started' ORDER BY CASE WHEN tc.call_index IS NULL THEN -1 ELSE tc.call_index END, ea.id",
        vec![invoking_attempt_id.into()],
    )).await?;
    if rows.is_empty() {
        return Ok(StartedEffectRecovery::None);
    }
    let effects: Vec<_> = rows
        .iter()
        .map(parse_started_effect)
        .collect::<StorageResult<_>>()?;
    validate_batch(&effects)?;
    let node_instance_id = effects[0].node_instance_id.clone();
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
    if !checkpoint.checksum_is_valid()
        || !effects
            .iter()
            .any(|effect| effect.effect_attempt_id == checkpoint.effect_watermark)
    {
        return Err(StorageError::Integrity(
            "started effect checkpoint watermark mismatch".into(),
        ));
    }
    for effect in &effects {
        recover_effect(connection, effect, error_object_id, &mut checkpoint, now).await?;
    }
    let coordinated = effects.iter().find(|effect| effect.requires_coordination());
    let wait_id = if coordinated.is_some() {
        let wait_id = allocate_wait_id(connection, &node_instance_id).await?;
        checkpoint.wait_ids.push(wait_id.clone());
        Some(wait_id)
    } else {
        None
    };
    checkpoint.effect_watermark = coordinated
        .unwrap_or_else(|| effects.last().expect("nonempty effects"))
        .effect_attempt_id
        .clone();
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await?;
    if let (Some(wait_id), Some(effect)) = (wait_id, coordinated) {
        open_coordination_wait(connection, &wait_id, invoking_attempt_id, effect, now).await?;
        Ok(StartedEffectRecovery::Waiting { count: 1, wait_id })
    } else {
        Ok(StartedEffectRecovery::RetryReady(
            u64::try_from(effects.len())
                .map_err(|_| StorageError::Integrity("started effect count overflow".into()))?,
        ))
    }
}

fn parse_started_effect(row: &sea_orm::QueryResult) -> StorageResult<StartedEffect> {
    let model_call_id: Option<String> = row.try_get("", "model_call_id")?;
    let count_call_id: Option<String> = row.try_get("", "count_call_id")?;
    let tool_call_id: Option<String> = row.try_get("", "tool_call_id")?;
    let owner = match (model_call_id, count_call_id, tool_call_id) {
        (Some(id), None, None) => StartedOwner::Model(id),
        (None, Some(id), None) => StartedOwner::Count(id),
        (None, None, Some(id)) => StartedOwner::Tool(id),
        _ => {
            return Err(StorageError::InvalidArgument(
                "started recovery owner is not supported".into(),
            ));
        }
    };
    Ok(StartedEffect {
        effect_attempt_id: row.try_get("", "effect_attempt_id")?,
        effect_id: row.try_get("", "effect_id")?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        classification: row.try_get("", "classification")?,
        owner,
    })
}

fn validate_batch(effects: &[StartedEffect]) -> StorageResult<()> {
    let node_instance_id = &effects[0].node_instance_id;
    if effects
        .iter()
        .any(|effect| effect.node_instance_id != *node_instance_id)
        || (effects.len() > 1
            && effects
                .iter()
                .any(|effect| !matches!(effect.owner, StartedOwner::Tool(_))))
        || effects
            .iter()
            .filter(|effect| effect.requires_coordination())
            .count()
            > 1
    {
        return Err(StorageError::Integrity(
            "started effect batch violates tool concurrency invariants".into(),
        ));
    }
    Ok(())
}

async fn recover_effect<C: ConnectionTrait>(
    connection: &C,
    effect: &StartedEffect,
    error_object_id: &str,
    checkpoint: &mut LlmLoopCheckpoint,
    now: i64,
) -> StorageResult<()> {
    let requires_coordination = effect.requires_coordination();
    let owner_status = if requires_coordination {
        "outcome_unknown"
    } else {
        "retry_ready"
    };
    let attempt = connection.execute(sql(
        "UPDATE effect_attempts SET status = 'outcome_unknown', error_object_id = ?, finished_at = ? WHERE id = ? AND status = 'started'",
        vec![error_object_id.into(), now.into(), effect.effect_attempt_id.clone().into()],
    )).await?;
    let owner = match &effect.owner {
        StartedOwner::Model(id) => {
            connection
                .execute(sql(
                    "UPDATE model_calls SET status = ? WHERE id = ? AND status = 'running'",
                    vec![owner_status.into(), id.clone().into()],
                ))
                .await?
        }
        StartedOwner::Count(id) => connection
            .execute(sql(
                "UPDATE count_calls SET status = 'retry_ready' WHERE id = ? AND status = 'running'",
                vec![id.clone().into()],
            ))
            .await?,
        StartedOwner::Tool(id) => {
            connection
                .execute(sql(
                    "UPDATE tool_calls SET status = ? WHERE id = ? AND status = 'running'",
                    vec![owner_status.into(), id.clone().into()],
                ))
                .await?
        }
    };
    if attempt.rows_affected() != 1 || owner.rows_affected() != 1 {
        return Err(StorageError::Conflict("started_effect_recovery"));
    }
    if requires_coordination
        && connection
            .execute(sql(
                "UPDATE effects SET status = 'outcome_unknown', completed_at = ? WHERE id = ? AND status = 'pending'",
                vec![now.into(), effect.effect_id.clone().into()],
            ))
            .await?
            .rows_affected()
            != 1
    {
        return Err(StorageError::Conflict("started_effect_recovery"));
    }
    update_checkpoint(effect, checkpoint)?;
    add_ref(
        connection,
        error_object_id,
        "effect_attempt",
        &effect.effect_attempt_id,
        "error",
        now,
    )
    .await
}

fn update_checkpoint(
    effect: &StartedEffect,
    checkpoint: &mut LlmLoopCheckpoint,
) -> StorageResult<()> {
    let requires_coordination = effect.requires_coordination();
    match &effect.owner {
        StartedOwner::Model(id) => {
            let active = checkpoint.active_model_effect.as_mut().ok_or_else(|| {
                StorageError::Integrity("started model effect checkpoint is missing".into())
            })?;
            if active.model_call_id != *id
                || active.effect_id != effect.effect_id
                || active.status != LlmLogicalCallStatus::Running
            {
                return Err(StorageError::Integrity(
                    "started model checkpoint projection mismatch".into(),
                ));
            }
            active.status = if requires_coordination {
                LlmLogicalCallStatus::OutcomeUnknown
            } else {
                LlmLogicalCallStatus::RetryReady
            };
            active.response_ref = None;
        }
        StartedOwner::Count(id) => {
            let active = checkpoint.active_count_effect.as_mut().ok_or_else(|| {
                StorageError::Integrity("started count effect checkpoint is missing".into())
            })?;
            if active.count_call_id != *id
                || active.effect_id != effect.effect_id
                || active.status != LlmLogicalCallStatus::Running
            {
                return Err(StorageError::Integrity(
                    "started count checkpoint projection mismatch".into(),
                ));
            }
            active.status = LlmLogicalCallStatus::RetryReady;
            active.result_source = None;
            active.result_ref = None;
        }
        StartedOwner::Tool(id) => {
            let active = checkpoint
                .current_batch
                .iter_mut()
                .find(|call| {
                    call.tool_call_id == *id
                        && call.effect_id.as_deref() == Some(effect.effect_id.as_str())
                })
                .ok_or_else(|| {
                    StorageError::Integrity("started tool effect checkpoint is missing".into())
                })?;
            if active.status != ToolCallCheckpointStatus::Running {
                return Err(StorageError::Integrity(
                    "started tool checkpoint projection mismatch".into(),
                ));
            }
            active.status = if requires_coordination {
                ToolCallCheckpointStatus::OutcomeUnknown
            } else {
                ToolCallCheckpointStatus::RetryReady
            };
            active.output_ref = None;
        }
    }
    Ok(())
}

async fn open_coordination_wait<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
    invoking_attempt_id: &str,
    effect: &StartedEffect,
    now: i64,
) -> StorageResult<()> {
    let owner = match &effect.owner {
        StartedOwner::Model(model_call_id) => EffectWaitOwner::Model { model_call_id },
        StartedOwner::Tool(tool_call_id) => EffectWaitOwner::Tool { tool_call_id },
        StartedOwner::Count(_) => {
            return Err(StorageError::Integrity(
                "count effect cannot require coordination".into(),
            ));
        }
    };
    open_effect_resolution_wait(
        connection,
        EffectWait {
            wait_id,
            node_instance_id: &effect.node_instance_id,
            invoking_node_attempt_id: invoking_attempt_id,
            owner,
            effect_id: &effect.effect_id,
            effect_attempt_id: &effect.effect_attempt_id,
            classification: &effect.classification,
        },
        now,
    )
    .await
}
