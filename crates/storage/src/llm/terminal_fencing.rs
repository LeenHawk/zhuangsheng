use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{LlmLogicalCallStatus, LlmLoopCheckpoint, ToolCallCheckpointStatus},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
    runtime::add_object_ref,
};

use super::model_ledger_helpers::persist_checkpoint;

pub(crate) async fn fence_run_effects<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    terminal_epoch: u64,
    now: i64,
) -> StorageResult<u64> {
    let rows = connection
        .query_all_raw(sql(
            "SELECT e.id AS effect_id, e.node_instance_id, e.model_call_id, e.count_call_id, e.tool_call_id FROM effects e JOIN node_instances ni ON ni.id = e.node_instance_id WHERE ni.run_id = ? AND e.status IN ('pending','outcome_unknown') ORDER BY e.id",
            vec![run_id.into()],
        ))
        .await?;
    for row in &rows {
        fence_effect(connection, row, run_id, terminal_epoch, now).await?;
    }
    let unmaterialized =
        fence_unmaterialized_tool_calls(connection, run_id, terminal_epoch, now).await?;
    u64::try_from(rows.len())
        .ok()
        .and_then(|count| count.checked_add(unmaterialized))
        .ok_or_else(|| StorageError::Integrity("terminal effect count overflow".into()))
}

async fn fence_unmaterialized_tool_calls<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    terminal_epoch: u64,
    now: i64,
) -> StorageResult<u64> {
    let rows = connection.query_all_raw(sql(
        "SELECT tc.id AS tool_call_id, tc.node_instance_id FROM tool_calls tc JOIN node_instances ni ON ni.id = tc.node_instance_id WHERE ni.run_id = ? AND tc.status IN ('requested','validated','awaiting_approval') AND NOT EXISTS (SELECT 1 FROM effects e WHERE e.tool_call_id = tc.id) ORDER BY tc.node_instance_id, tc.call_index",
        vec![run_id.into()],
    )).await?;
    for row in &rows {
        let tool_call_id: String = row.try_get("", "tool_call_id")?;
        let node_instance_id: String = row.try_get("", "node_instance_id")?;
        if connection.execute_raw(sql(
            "UPDATE tool_calls SET status = 'cancelled_before_start', finished_at = ? WHERE id = ? AND status IN ('requested','validated','awaiting_approval')",
            vec![now.into(), tool_call_id.clone().into()],
        )).await?.rows_affected() != 1 {
            return Err(StorageError::Conflict("terminal_tool_owner"));
        }
        update_unmaterialized_checkpoint(
            connection,
            &node_instance_id,
            &tool_call_id,
            run_id,
            terminal_epoch,
            now,
        )
        .await?;
        abort_open_tool_blocker(connection, &tool_call_id, run_id, terminal_epoch, now).await?;
    }
    u64::try_from(rows.len())
        .map_err(|_| StorageError::Integrity("terminal tool count overflow".into()))
}

async fn update_unmaterialized_checkpoint<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
    tool_call_id: &str,
    run_id: &str,
    terminal_epoch: u64,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("terminal tool checkpoint missing".into()))?;
    let mut checkpoint: LlmLoopCheckpoint = load_object_json(
        connection,
        &row.try_get::<String>("", "checkpoint_object_id")?,
    )
    .await?;
    if !checkpoint.checksum_is_valid() {
        return Err(StorageError::Integrity(
            "terminal tool checkpoint checksum is invalid".into(),
        ));
    }
    let call = checkpoint
        .current_batch
        .iter_mut()
        .find(|call| call.tool_call_id == tool_call_id && call.effect_id.is_none())
        .ok_or_else(|| StorageError::Integrity("terminal tool checkpoint is missing".into()))?;
    call.status = ToolCallCheckpointStatus::CancelledBeforeStart;
    call.output_ref = None;
    call.wait_id = None;
    checkpoint.effect_watermark = format!("run-terminal:{run_id}:{terminal_epoch}");
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await
}

async fn abort_open_tool_blocker<C: ConnectionTrait>(
    connection: &C,
    tool_call_id: &str,
    run_id: &str,
    terminal_epoch: u64,
    now: i64,
) -> StorageResult<()> {
    let blocker = connection.query_one_raw(sql(
        "SELECT wb.wait_id,wb.blocker_kind,wb.blocker_id FROM wait_blockers wb LEFT JOIN memory_proposal_tool_calls mt ON wb.blocker_kind='memory_proposal' AND mt.proposal_id=wb.blocker_id WHERE wb.status='open' AND ((wb.blocker_kind='tool_call' AND wb.blocker_id=?) OR mt.tool_call_id=?)",
        vec![tool_call_id.into(),tool_call_id.into()],
    )).await?;
    let Some(blocker) = blocker else {
        return Ok(());
    };
    let wait_id: String = blocker.try_get("", "wait_id")?;
    let blocker_kind: String = blocker.try_get("", "blocker_kind")?;
    let blocker_id: String = blocker.try_get("", "blocker_id")?;
    let decision = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": "run_terminal_cancel_before_start",
        "runId": run_id,
        "terminalEpoch": terminal_epoch,
        "toolCallId": tool_call_id,
        "blockerKind": blocker_kind,
        "blockerId": blocker_id,
    }))?;
    let decision_id = put_inline_object(connection, &decision, now).await?;
    if connection.execute_raw(sql(
        "UPDATE wait_blockers SET status = 'aborted', decision_object_id = ? WHERE wait_id = ? AND blocker_kind = ? AND blocker_id = ? AND status = 'open'",
        vec![decision_id.clone().into(), wait_id.clone().into(), blocker_kind.into(), blocker_id.into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("terminal_tool_blocker"));
    }
    add_object_ref(
        connection,
        &decision_id,
        "node_wait",
        &wait_id,
        "terminal_decision",
        now,
    )
    .await
}

async fn fence_effect<C: ConnectionTrait>(
    connection: &C,
    row: &sea_orm::QueryResult,
    run_id: &str,
    terminal_epoch: u64,
    now: i64,
) -> StorageResult<()> {
    let effect_id: String = row.try_get("", "effect_id")?;
    let unresolved = connection
        .query_one_raw(sql(
            "SELECT ea.id, ea.status FROM effect_attempts ea LEFT JOIN effect_resolutions er ON er.effect_attempt_id = ea.id WHERE ea.effect_id = ? AND ea.status IN ('started','outcome_unknown') AND er.id IS NULL ORDER BY ea.attempt_no DESC LIMIT 1",
            vec![effect_id.clone().into()],
        ))
        .await?;
    let (owner_status, checkpoint_status, resolution_kind) = if let Some(attempt) = unresolved {
        let attempt_id: String = attempt.try_get("", "id")?;
        if attempt.try_get::<String>("", "status")? == "started" {
            let updated = connection
                .execute_raw(sql(
                    "UPDATE effect_attempts SET status = 'outcome_unknown', finished_at = ? WHERE id = ? AND status = 'started'",
                    vec![now.into(), attempt_id.clone().into()],
                ))
                .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict("terminal_effect_attempt"));
            }
        }
        write_system_resolution(
            connection,
            &effect_id,
            &attempt_id,
            run_id,
            terminal_epoch,
            "run_terminal_abandon",
            now,
        )
        .await?;
        (
            "abandoned_unknown",
            LlmLogicalCallStatus::AbandonedUnknown,
            "run_terminal_abandon",
        )
    } else {
        let prepared = connection
            .query_all_raw(sql(
                "SELECT ea.id FROM effect_attempts ea LEFT JOIN effect_resolutions er ON er.effect_attempt_id = ea.id WHERE ea.effect_id = ? AND ea.status = 'prepared' AND er.id IS NULL ORDER BY ea.attempt_no",
                vec![effect_id.clone().into()],
            ))
            .await?;
        for attempt in prepared {
            let attempt_id: String = attempt.try_get("", "id")?;
            let updated = connection
                .execute_raw(sql(
                    "UPDATE effect_attempts SET status = 'superseded_before_start', finished_at = ? WHERE id = ? AND status = 'prepared'",
                    vec![now.into(), attempt_id.clone().into()],
                ))
                .await?;
            if updated.rows_affected() != 1 {
                return Err(StorageError::Conflict("terminal_effect_attempt"));
            }
            write_system_resolution(
                connection,
                &effect_id,
                &attempt_id,
                run_id,
                terminal_epoch,
                "run_terminal_cancel_before_start",
                now,
            )
            .await?;
        }
        (
            "cancelled_before_start",
            LlmLogicalCallStatus::CancelledBeforeStart,
            "run_terminal_cancel_before_start",
        )
    };
    let effect = connection
        .execute_raw(sql(
            "UPDATE effects SET status = ?, completed_at = ? WHERE id = ? AND status IN ('pending','outcome_unknown')",
            vec![owner_status.into(), now.into(), effect_id.clone().into()],
        ))
        .await?;
    if effect.rows_affected() != 1 {
        return Err(StorageError::Conflict("terminal_effect_projection"));
    }
    update_owner_and_checkpoint(
        connection,
        row,
        &effect_id,
        owner_status,
        checkpoint_status,
        now,
    )
    .await?;
    abort_open_blocker(
        connection,
        &effect_id,
        run_id,
        terminal_epoch,
        resolution_kind,
        now,
    )
    .await
}

async fn update_owner_and_checkpoint<C: ConnectionTrait>(
    connection: &C,
    row: &sea_orm::QueryResult,
    effect_id: &str,
    owner_status: &str,
    checkpoint_status: LlmLogicalCallStatus,
    now: i64,
) -> StorageResult<()> {
    let node_instance_id: String = row.try_get("", "node_instance_id")?;
    let model_call_id: Option<String> = row.try_get("", "model_call_id")?;
    let count_call_id: Option<String> = row.try_get("", "count_call_id")?;
    let tool_call_id: Option<String> = row.try_get("", "tool_call_id")?;
    let checkpoint_row = connection
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("terminal effect checkpoint missing".into()))?;
    let mut checkpoint: LlmLoopCheckpoint = load_object_json(
        connection,
        &checkpoint_row.try_get::<String>("", "checkpoint_object_id")?,
    )
    .await?;
    if !checkpoint.checksum_is_valid() {
        return Err(StorageError::Integrity(
            "terminal effect checkpoint checksum is invalid".into(),
        ));
    }
    match (model_call_id, count_call_id, tool_call_id) {
        (Some(owner_id), None, None) => {
            update_owner(connection, "model_calls", &owner_id, owner_status).await?;
            let active = checkpoint.active_model_effect.as_mut().ok_or_else(|| {
                StorageError::Integrity("terminal model checkpoint is missing".into())
            })?;
            if active.model_call_id != owner_id || active.effect_id != effect_id {
                return Err(StorageError::Integrity(
                    "terminal model checkpoint owner mismatch".into(),
                ));
            }
            active.status = checkpoint_status;
            active.response_ref = None;
        }
        (None, Some(owner_id), None) => {
            update_owner(connection, "count_calls", &owner_id, owner_status).await?;
            let active = checkpoint.active_count_effect.as_mut().ok_or_else(|| {
                StorageError::Integrity("terminal count checkpoint is missing".into())
            })?;
            if active.count_call_id != owner_id || active.effect_id != effect_id {
                return Err(StorageError::Integrity(
                    "terminal count checkpoint owner mismatch".into(),
                ));
            }
            active.status = checkpoint_status;
            active.result_ref = None;
        }
        (None, None, Some(owner_id)) => {
            update_owner(connection, "tool_calls", &owner_id, owner_status).await?;
            let active = checkpoint
                .current_batch
                .iter_mut()
                .find(|call| {
                    call.tool_call_id == owner_id && call.effect_id.as_deref() == Some(effect_id)
                })
                .ok_or_else(|| {
                    StorageError::Integrity("terminal tool checkpoint is missing".into())
                })?;
            active.status = match checkpoint_status {
                LlmLogicalCallStatus::CancelledBeforeStart => {
                    ToolCallCheckpointStatus::CancelledBeforeStart
                }
                _ => ToolCallCheckpointStatus::AbandonedUnknown,
            };
            active.output_ref = None;
        }
        _ => {
            return Err(StorageError::Integrity(
                "terminal effect owner association is invalid".into(),
            ));
        }
    }
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await
}

async fn update_owner<C: ConnectionTrait>(
    connection: &C,
    table: &str,
    owner_id: &str,
    status: &str,
) -> StorageResult<()> {
    let statement = match table {
        "model_calls" => {
            "UPDATE model_calls SET status = ? WHERE id = ? AND status IN ('prepared','running','outcome_unknown','retry_ready')"
        }
        "count_calls" => {
            "UPDATE count_calls SET status = ? WHERE id = ? AND status IN ('prepared','running','retry_ready')"
        }
        "tool_calls" => {
            "UPDATE tool_calls SET status = ? WHERE id = ? AND status IN ('requested','validated','awaiting_approval','prepared','running','outcome_unknown','retry_ready')"
        }
        _ => {
            return Err(StorageError::Integrity(
                "unknown terminal effect owner".into(),
            ));
        }
    };
    if connection
        .execute_raw(sql(statement, vec![status.into(), owner_id.into()]))
        .await?
        .rows_affected()
        != 1
    {
        return Err(StorageError::Conflict("terminal_effect_owner"));
    }
    Ok(())
}

async fn write_system_resolution<C: ConnectionTrait>(
    connection: &C,
    effect_id: &str,
    attempt_id: &str,
    run_id: &str,
    terminal_epoch: u64,
    kind: &str,
    now: i64,
) -> StorageResult<()> {
    let decision = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": kind,
        "runId": run_id,
        "terminalEpoch": terminal_epoch,
    }))?;
    let decision_id = put_inline_object(connection, &decision, now).await?;
    let key = format!("run-terminal:{run_id}:{terminal_epoch}:{attempt_id}");
    let digest = canonical::hash(&json!({
        "effectId": effect_id,
        "effectAttemptId": attempt_id,
        "kind": kind,
        "key": key,
    }))?;
    connection
        .execute_raw(sql(
            "INSERT INTO effect_resolutions (id, effect_id, effect_attempt_id, resolution_kind, command_idempotency_key, request_digest, decision_object_id, actor_kind, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'system', ?)",
            vec![
                new_id("effectresolution").into(),
                effect_id.into(),
                attempt_id.into(),
                kind.into(),
                key.into(),
                digest.into(),
                decision_id.clone().into(),
                now.into(),
            ],
        ))
        .await?;
    add_object_ref(
        connection,
        &decision_id,
        "effect_attempt",
        attempt_id,
        "terminal_resolution",
        now,
    )
    .await
}

async fn abort_open_blocker<C: ConnectionTrait>(
    connection: &C,
    effect_id: &str,
    run_id: &str,
    terminal_epoch: u64,
    kind: &str,
    now: i64,
) -> StorageResult<()> {
    let blocker = connection
        .query_one_raw(sql(
            "SELECT wait_id FROM wait_blockers WHERE blocker_kind = 'effect' AND blocker_id = ? AND status = 'open'",
            vec![effect_id.into()],
        ))
        .await?;
    let Some(blocker) = blocker else {
        return Ok(());
    };
    let wait_id: String = blocker.try_get("", "wait_id")?;
    let decision = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": kind,
        "runId": run_id,
        "terminalEpoch": terminal_epoch,
    }))?;
    let decision_id = put_inline_object(connection, &decision, now).await?;
    connection
        .execute_raw(sql(
            "UPDATE wait_blockers SET status = 'aborted', decision_object_id = ? WHERE wait_id = ? AND blocker_kind = 'effect' AND blocker_id = ? AND status = 'open'",
            vec![decision_id.clone().into(), wait_id.clone().into(), effect_id.into()],
        ))
        .await?;
    add_object_ref(
        connection,
        &decision_id,
        "node_wait",
        &wait_id,
        "terminal_decision",
        now,
    )
    .await
}
