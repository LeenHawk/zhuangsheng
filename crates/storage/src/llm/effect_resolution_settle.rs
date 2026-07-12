use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{EffectResolutionKind, ResolveEffectUnknownCommand},
};

use crate::{
    StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{new_id, put_inline_object, sql},
    },
    runtime::{Event, add_object_ref, append_event, copy_attempt_reads, enqueue_wakeup},
};

use super::{effect_resolution_helpers::ResolutionContext, terminal_fencing::fence_run_effects};

pub(super) async fn settle_blocker<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    decision_object_id: &str,
    now: i64,
) -> StorageResult<()> {
    let blocker_status = if command.kind == EffectResolutionKind::AbortRun {
        "aborted"
    } else {
        "satisfied"
    };
    let blocker = connection
        .execute_raw(sql(
            "UPDATE wait_blockers SET status = ?, decision_object_id = ? WHERE wait_id = ? AND blocker_kind = 'effect' AND blocker_id = ? AND status = 'open'",
            vec![
                blocker_status.into(),
                decision_object_id.into(),
                context.wait_id.clone().into(),
                command.effect_id.clone().into(),
            ],
        ))
        .await?;
    if blocker.rows_affected() != 1 {
        return Err(StorageError::Conflict("effect_wait_blocker"));
    }
    if command.kind == EffectResolutionKind::AbortRun {
        abort_run(connection, command, context, now).await
    } else {
        resume_instance(connection, command, context, decision_object_id, now).await
    }
}

async fn resume_instance<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    decision_object_id: &str,
    now: i64,
) -> StorageResult<()> {
    let response = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": "effect_resolution",
        "effectId": command.effect_id,
        "effectAttemptId": command.expected_effect_attempt_id,
        "resolutionId": command.resolution_id,
        "resolutionKind": command.kind,
        "decisionRef": decision_object_id,
        "resultRef": command.result_object_id,
        "evidenceRef": command.evidence_object_id,
    }))?;
    let response_object_id = put_inline_object(connection, &response, now).await?;
    let wait = connection
        .execute_raw(sql(
            "UPDATE node_waits SET status = 'resolved', response_object_id = ?, accepted_delivery_id = ?, resolved_at = ? WHERE id = ? AND status = 'open'",
            vec![
                response_object_id.clone().into(),
                format!("effect-resolution:{}", command.resolution_id).into(),
                now.into(),
                context.wait_id.clone().into(),
            ],
        ))
        .await?;
    let instance = connection
        .execute_raw(sql(
            "UPDATE node_instances SET status = 'ready', updated_at = ? WHERE id = ? AND status = 'waiting'",
            vec![now.into(), context.node_instance_id.clone().into()],
        ))
        .await?;
    if wait.rows_affected() != 1 || instance.rows_affected() != 1 {
        return Err(StorageError::Conflict("effect_wait_settle"));
    }
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET open_waits = open_waits - 1 WHERE run_id = ? AND open_waits > 0",
            vec![context.run_id.clone().into()],
        ))
        .await?;
    let resume_attempt_id = create_resume_attempt(connection, command, context, now).await?;
    let seq = append_event(
        connection,
        Event {
            run_id: &context.run_id,
            event_type: "effect.resolved",
            importance: "critical",
            node_instance_id: Some(&context.node_instance_id),
            attempt_id: Some(&context.invoking_node_attempt_id),
            payload: json!({
                "schemaVersion": 1,
                "effectId": command.effect_id,
                "effectAttemptId": command.expected_effect_attempt_id,
                "resolutionId": command.resolution_id,
                "resolutionKind": command.kind,
                "waitId": context.wait_id,
                "resumeAttemptId": resume_attempt_id,
            }),
            now,
        },
    )
    .await?;
    if context.run_status == "running" {
        enqueue_wakeup(
            connection,
            &context.run_id,
            Some(&context.node_id),
            "attempt_ready",
            seq,
            &format!("effect-resolution-resume:{}", command.resolution_id),
            now,
        )
        .await?;
    }
    add_object_ref(
        connection,
        &response_object_id,
        "node_wait",
        &context.wait_id,
        "response",
        now,
    )
    .await
}

async fn create_resume_attempt<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    now: i64,
) -> StorageResult<String> {
    let row = connection
        .query_one_raw(sql(
            "SELECT a.executor_object_id, a.retry_ordinal, ni.graph_revision_id, COALESCE(MAX(all_attempts.attempt_no), 0) AS max_attempt_no FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id LEFT JOIN node_attempts all_attempts ON all_attempts.node_instance_id = ni.id WHERE a.id = ? GROUP BY a.id, ni.id",
            vec![context.wait_attempt_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("wait attempt is unavailable".into()))?;
    let revision_id: String = row.try_get("", "graph_revision_id")?;
    let revision = load_revision(connection, &revision_id).await?;
    let max_attempt_no: i64 = row.try_get("", "max_attempt_no")?;
    let next_attempt_no = max_attempt_no
        .checked_add(1)
        .ok_or_else(|| StorageError::Integrity("node attempt number overflow".into()))?;
    if u64::try_from(next_attempt_no).ok()
        > Some(revision.definition.limits.max_attempts_per_activation)
    {
        return Err(StorageError::InvalidArgument(
            "node attempt limit prevents effect resolution resume".into(),
        ));
    }
    let attempt_id = new_id("attempt");
    connection
        .execute_raw(sql(
            "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) VALUES (?, ?, ?, ?, 'resume', 'queued', ?, 0, ?, ?)",
            vec![
                attempt_id.clone().into(),
                context.node_instance_id.clone().into(),
                next_attempt_no.into(),
                row.try_get::<i64>("", "retry_ordinal")?.into(),
                i64::try_from(context.control_epoch)
                    .map_err(|_| StorageError::Integrity("run control epoch overflow".into()))?
                    .into(),
                format!("effect-resolution:{}:resume", command.resolution_id).into(),
                row.try_get::<String>("", "executor_object_id")?.into(),
            ],
        ))
        .await?;
    copy_attempt_reads(connection, &context.wait_attempt_id, &attempt_id, now).await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET total_attempts = total_attempts + 1 WHERE run_id = ?",
            vec![context.run_id.clone().into()],
        ))
        .await?;
    Ok(attempt_id)
}

async fn abort_run<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    now: i64,
) -> StorageResult<()> {
    let updated = connection
        .execute_raw(sql(
            "UPDATE graph_runs SET status = 'cancelled', control_epoch = control_epoch + 1, drain_epoch = NULL, finished_at = ?, updated_at = ? WHERE id = ? AND control_epoch = ? AND status NOT IN ('completed','failed','cancelled')",
            vec![
                now.into(),
                now.into(),
                context.run_id.clone().into(),
                i64::try_from(context.control_epoch)
                    .map_err(|_| StorageError::Integrity("run control epoch overflow".into()))?
                    .into(),
            ],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_control_epoch"));
    }
    fence_run_effects(
        connection,
        &context.run_id,
        context.control_epoch.saturating_add(1),
        now,
    )
    .await?;
    connection.execute_raw(sql(
        "UPDATE node_attempts SET status = 'cancelled', worker_id = NULL, lease_until = NULL, finished_at = ? WHERE node_instance_id IN (SELECT id FROM node_instances WHERE run_id = ?) AND status IN ('queued','leased','running','waiting')",
        vec![now.into(), context.run_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE node_instances SET status = 'cancelled', updated_at = ? WHERE run_id = ? AND status IN ('ready','running','waiting')",
        vec![now.into(), context.run_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE node_waits SET status = 'cancelled', resolved_at = ? WHERE run_id = ? AND status = 'open'",
        vec![now.into(), context.run_id.clone().into()],
    )).await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET open_waits = 0, coordinator_buffered_values = 0 WHERE run_id = ?",
            vec![context.run_id.clone().into()],
        ))
        .await?;
    connection.execute_raw(sql(
        "UPDATE coordination_buffer_items SET status = 'cancelled', terminal_at = ? WHERE run_id = ? AND status = 'indexed'",
        vec![now.into(), context.run_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE aggregation_windows SET status = 'cancelled', closed_at = ? WHERE run_id = ? AND status = 'open'",
        vec![now.into(), context.run_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE run_id = ? AND status IN ('pending','claimed')",
        vec![context.run_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE run_id = ? AND status IN ('pending','ready')",
        vec![context.run_id.clone().into()],
    )).await?;
    append_event(
        connection,
        Event {
            run_id: &context.run_id,
            event_type: "effect.resolved",
            importance: "critical",
            node_instance_id: Some(&context.node_instance_id),
            attempt_id: Some(&context.invoking_node_attempt_id),
            payload: json!({
                "schemaVersion": 1,
                "effectId": command.effect_id,
                "resolutionId": command.resolution_id,
                "resolutionKind": command.kind,
                "waitId": context.wait_id,
            }),
            now,
        },
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id: &context.run_id,
            event_type: "run.cancelled",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion": 1, "reason": "effect_resolution_abort"}),
            now,
        },
    )
    .await?;
    Ok(())
}
