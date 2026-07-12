use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::canonical;

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
    runtime::{Event, add_object_ref, append_event},
};

pub(super) enum EffectWaitOwner<'a> {
    Model { model_call_id: &'a str },
    Tool { tool_call_id: &'a str },
}

pub(super) struct EffectWait<'a> {
    pub wait_id: &'a str,
    pub node_instance_id: &'a str,
    pub invoking_node_attempt_id: &'a str,
    pub owner: EffectWaitOwner<'a>,
    pub effect_id: &'a str,
    pub effect_attempt_id: &'a str,
    pub classification: &'a str,
}

pub(super) async fn open_effect_resolution_wait<C: ConnectionTrait>(
    connection: &C,
    wait: EffectWait<'_>,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ni.run_id, ni.node_id, cp.checkpoint_object_id FROM node_instances ni JOIN llm_loop_checkpoints cp ON cp.node_instance_id = ni.id WHERE ni.id = ?",
            vec![wait.node_instance_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("effect wait owner is unavailable".into()))?;
    let run_id: String = row.try_get("", "run_id")?;
    let node_id: String = row.try_get("", "node_id")?;
    let continuation_object_id: String = row.try_get("", "checkpoint_object_id")?;
    let (owner_kind, owner_id) = match wait.owner {
        EffectWaitOwner::Model { model_call_id } => ("model_call", model_call_id),
        EffectWaitOwner::Tool { tool_call_id } => ("tool_call", tool_call_id),
    };
    let request = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": "effect_resolution",
        "effectId": wait.effect_id,
        "effectAttemptId": wait.effect_attempt_id,
        "ownerKind": owner_kind,
        "ownerId": owner_id,
        "classification": wait.classification,
        "allowedResolutions": [
            "confirm_succeeded",
            "confirm_failed_retry_safe",
            "abort_run"
        ]
    }))?;
    let request_object_id = put_inline_object(connection, &request, now).await?;
    connection
        .execute_raw(sql(
            "INSERT INTO node_waits (id, run_id, node_instance_id, node_attempt_id, kind, correlation_key, request_object_id, continuation_object_id, on_timeout, status, created_at) VALUES (?, ?, ?, ?, 'effect_resolution', ?, ?, ?, 'fail', 'open', ?)",
            vec![
                wait.wait_id.into(),
                run_id.clone().into(),
                wait.node_instance_id.into(),
                wait.invoking_node_attempt_id.into(),
                format!("effect:{}", wait.effect_id).into(),
                request_object_id.clone().into(),
                continuation_object_id.clone().into(),
                now.into(),
            ],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "INSERT INTO wait_blockers (wait_id, blocker_kind, blocker_id, blocker_order, status) VALUES (?, 'effect', ?, 0, 'open')",
            vec![wait.wait_id.into(), wait.effect_id.into()],
        ))
        .await?;
    let attempt = connection
        .execute_raw(sql(
            "UPDATE node_attempts SET status = 'waiting', continuation_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND node_instance_id = ? AND status = 'running'",
            vec![
                continuation_object_id.clone().into(),
                now.into(),
                wait.invoking_node_attempt_id.into(),
                wait.node_instance_id.into(),
            ],
        ))
        .await?;
    let instance = connection
        .execute_raw(sql(
            "UPDATE node_instances SET status = 'waiting', updated_at = ? WHERE id = ? AND status = 'running'",
            vec![now.into(), wait.node_instance_id.into()],
        ))
        .await?;
    if attempt.rows_affected() != 1 || instance.rows_affected() != 1 {
        return Err(StorageError::Conflict("effect_wait_owner_status"));
    }
    connection
        .execute_raw(sql(
            "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
            vec![wait.invoking_node_attempt_id.into()],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE run_id = ? AND node_id = ? AND kind = 'attempt_ready' AND status = 'claimed'",
            vec![run_id.clone().into(), node_id.clone().into()],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET open_waits = open_waits + 1 WHERE run_id = ?",
            vec![run_id.clone().into()],
        ))
        .await?;
    add_object_ref(
        connection,
        &request_object_id,
        "node_wait",
        wait.wait_id,
        "request",
        now,
    )
    .await?;
    add_object_ref(
        connection,
        &continuation_object_id,
        "node_wait",
        wait.wait_id,
        "continuation",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id: &run_id,
            event_type: "effect.outcome_unknown",
            importance: "critical",
            node_instance_id: Some(wait.node_instance_id),
            attempt_id: Some(wait.invoking_node_attempt_id),
            payload: json!({
                "schemaVersion": 1,
                "waitId": wait.wait_id,
                "effectId": wait.effect_id,
                "effectAttemptId": wait.effect_attempt_id,
                "nodeId": node_id
            }),
            now,
        },
    )
    .await?;
    Ok(())
}

pub(super) async fn allocate_wait_id<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
) -> StorageResult<String> {
    if connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM node_waits WHERE node_instance_id = ? AND status = 'open'",
            vec![node_instance_id.into()],
        ))
        .await?
        .is_some()
    {
        return Err(StorageError::Conflict("node_instance_open_wait"));
    }
    Ok(new_id("wait"))
}

pub(super) async fn load_wait_ids<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
) -> StorageResult<Vec<String>> {
    connection
        .query_all_raw(sql(
            "SELECT id FROM node_waits WHERE node_instance_id = ? ORDER BY created_at, id",
            vec![node_instance_id.into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "id").map_err(Into::into))
        .collect()
}
