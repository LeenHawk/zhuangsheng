use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::ToolFailureAction,
    llm::LlmLoopCheckpoint,
    runtime::{
        SubmitWaitResponseCommand, ToolApprovalDecisionKind, WaitDeliveryStatus, WaitDeliveryView,
        WaitResponsePayload,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    runtime::{Event, add_object_ref, append_event, enqueue_wakeup, fail_run},
};

use super::{
    tool_approval::ToolApprovalContinuation,
    wait_delivery_settle::{settle_executable_batch, settle_failed_batch},
    wait_delivery_validation::{settle_decisions, validate_continuation, validate_decisions},
};

pub(super) struct WaitContext {
    pub run_id: String,
    pub run_status: String,
    pub control_epoch: u64,
    pub node_instance_id: String,
    pub node_id: String,
    pub node_attempt_id: String,
    pub continuation_ref: String,
    kind: String,
    wait_status: String,
    instance_status: String,
}

impl SqliteStore {
    pub async fn submit_wait_response(
        &self,
        command: SubmitWaitResponseCommand,
        now: i64,
    ) -> StorageResult<WaitDeliveryView> {
        validate_command(&command)?;
        let payload_digest = canonical::hash(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(replay) = replay_delivery(&transaction, &command, &payload_digest).await? {
            transaction.commit().await?;
            return Ok(replay);
        }
        let context = load_wait_context(&transaction, &command.wait_id).await?;
        if matches!(
            context.run_status.as_str(),
            "completed" | "failed" | "cancelled"
        ) {
            return Err(StorageError::Conflict("run_terminal"));
        }
        if context.wait_status != "open" {
            return Err(StorageError::Conflict("wait_already_resolved"));
        }
        if has_open_effect_blocker(&transaction, &command.wait_id).await? {
            return Err(StorageError::Conflict("effect_resolution_required"));
        }
        if context.kind != "approval" || context.instance_status != "waiting" {
            return Err(StorageError::Conflict("wait_response_kind"));
        }
        let decisions = match &command.payload {
            WaitResponsePayload::ToolApproval { decisions } => decisions,
        };
        let continuation: ToolApprovalContinuation =
            load_object_json(&transaction, &context.continuation_ref).await?;
        validate_continuation(&transaction, &command, &context, &continuation).await?;
        let ordered = validate_decisions(
            &transaction,
            &command.wait_id,
            decisions,
            &continuation,
            now,
        )
        .await?;
        let fail_node = ordered.iter().any(|decision| {
            decision.decision == ToolApprovalDecisionKind::Reject
                && continuation
                    .calls
                    .iter()
                    .find(|call| call.tool_call_id == decision.tool_call_id)
                    .is_some_and(|call| call.denied_action == ToolFailureAction::FailNode)
        });
        let decision_refs =
            settle_decisions(&transaction, &command, &continuation, &ordered, now).await?;
        let mut checkpoint = load_current_checkpoint(
            &transaction,
            &continuation,
            &context.node_instance_id,
            &command.wait_id,
        )
        .await?;
        let (prepared, denied, resume_attempt_id) = if fail_node {
            settle_failed_batch(
                &transaction,
                &context,
                &continuation,
                &ordered,
                &mut checkpoint,
                now,
            )
            .await?
        } else {
            settle_executable_batch(
                &transaction,
                &context,
                &command.delivery_id,
                &continuation,
                &ordered,
                &mut checkpoint,
                now,
            )
            .await?
        };
        let response_ref = persist_wait_response(
            &transaction,
            &command,
            &decision_refs,
            &prepared,
            &denied,
            now,
        )
        .await?;
        let view = WaitDeliveryView {
            wait_id: command.wait_id.clone(),
            delivery_id: command.delivery_id.clone(),
            status: WaitDeliveryStatus::Resolved,
            prepared_tool_call_ids: prepared,
            denied_tool_call_ids: denied,
            replayed: false,
        };
        persist_delivery(
            &transaction,
            &command,
            &payload_digest,
            &response_ref,
            &view,
            now,
        )
        .await?;
        append_resolution_event(
            &transaction,
            &context,
            &command,
            resume_attempt_id.as_deref(),
            now,
        )
        .await?;
        if fail_node {
            fail_run(
                &transaction,
                &context.run_id,
                "tool_approval_rejected",
                "tool approval rejection failed the node",
                now,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(view)
    }
}

async fn load_wait_context<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
) -> StorageResult<WaitContext> {
    let row = connection.query_one_raw(sql(
        "SELECT w.kind, w.status AS wait_status, w.run_id, w.node_instance_id, w.node_attempt_id, w.continuation_object_id, r.status AS run_status, r.control_epoch, ni.node_id, ni.status AS instance_status FROM node_waits w JOIN graph_runs r ON r.id = w.run_id JOIN node_instances ni ON ni.id = w.node_instance_id WHERE w.id = ?",
        vec![wait_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "wait", id: wait_id.into() })?;
    Ok(WaitContext {
        run_id: row.try_get("", "run_id")?,
        run_status: row.try_get("", "run_status")?,
        control_epoch: u64::try_from(row.try_get::<i64>("", "control_epoch")?)
            .map_err(|_| StorageError::Integrity("invalid run control epoch".into()))?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        node_id: row.try_get("", "node_id")?,
        node_attempt_id: row.try_get("", "node_attempt_id")?,
        continuation_ref: row.try_get("", "continuation_object_id")?,
        kind: row.try_get("", "kind")?,
        wait_status: row.try_get("", "wait_status")?,
        instance_status: row.try_get("", "instance_status")?,
    })
}

async fn has_open_effect_blocker<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
) -> StorageResult<bool> {
    Ok(connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM wait_blockers WHERE wait_id = ? AND blocker_kind = 'effect' AND status = 'open'",
            vec![wait_id.into()],
        ))
        .await?
        .is_some())
}

async fn load_current_checkpoint<C: ConnectionTrait>(
    connection: &C,
    continuation: &ToolApprovalContinuation,
    node_instance_id: &str,
    wait_id: &str,
) -> StorageResult<LlmLoopCheckpoint> {
    let row = connection.query_one_raw(sql(
        "SELECT checkpoint_object_id, checkpoint_digest FROM llm_loop_checkpoints WHERE node_instance_id = ?",
        vec![node_instance_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("approval checkpoint is missing".into()))?;
    let object_id: String = row.try_get("", "checkpoint_object_id")?;
    if object_id != continuation.checkpoint_ref
        || row.try_get::<String>("", "checkpoint_digest")? != continuation.checkpoint_digest
    {
        return Err(StorageError::Conflict("approval_checkpoint_changed"));
    }
    let checkpoint: LlmLoopCheckpoint = load_object_json(connection, &object_id).await?;
    let checkpoint_calls: BTreeSet<_> = checkpoint
        .current_batch
        .iter()
        .filter(|call| call.wait_id.as_deref() == Some(wait_id))
        .map(|call| call.tool_call_id.as_str())
        .collect();
    let planned_calls: BTreeSet<_> = continuation
        .calls
        .iter()
        .map(|call| call.tool_call_id.as_str())
        .collect();
    if !checkpoint.checksum_is_valid()
        || !checkpoint.wait_ids.iter().any(|id| id == wait_id)
        || checkpoint_calls != planned_calls
    {
        return Err(StorageError::Integrity(
            "approval checkpoint is invalid".into(),
        ));
    }
    Ok(checkpoint)
}

async fn persist_wait_response<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    decision_refs: &[String],
    prepared: &[String],
    denied: &[String],
    now: i64,
) -> StorageResult<String> {
    let response = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": "tool_approval",
        "decisionRefs": decision_refs,
        "preparedToolCallIds": prepared,
        "deniedToolCallIds": denied,
    }))?;
    let response_ref = put_inline_object(connection, &response, now).await?;
    if connection.execute_raw(sql(
        "UPDATE node_waits SET status = 'resolved', response_object_id = ?, accepted_delivery_id = ?, resolved_at = ? WHERE id = ? AND status = 'open'",
        vec![response_ref.clone().into(), command.delivery_id.clone().into(), now.into(), command.wait_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("approval_wait_settle"));
    }
    add_object_ref(
        connection,
        &response_ref,
        "node_wait",
        &command.wait_id,
        "response",
        now,
    )
    .await?;
    Ok(response_ref)
}

async fn persist_delivery<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    payload_digest: &str,
    response_ref: &str,
    view: &WaitDeliveryView,
    now: i64,
) -> StorageResult<()> {
    let result_ref = put_inline_object(connection, &canonical::to_vec(view)?, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO wait_deliveries (wait_id, delivery_id, payload_digest, result_object_id, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![command.wait_id.clone().into(), command.delivery_id.clone().into(), payload_digest.into(), result_ref.clone().into(), now.into()],
    )).await?;
    add_object_ref(
        connection,
        &result_ref,
        "node_wait",
        &command.wait_id,
        "delivery_result",
        now,
    )
    .await?;
    add_object_ref(
        connection,
        response_ref,
        "wait_delivery",
        &command.delivery_id,
        "response",
        now,
    )
    .await?;
    connection.execute_raw(sql(
        "UPDATE run_execution_counters SET open_waits = open_waits - 1 WHERE run_id = (SELECT run_id FROM node_waits WHERE id = ?) AND open_waits > 0",
        vec![command.wait_id.clone().into()],
    )).await?;
    Ok(())
}

async fn append_resolution_event<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    command: &SubmitWaitResponseCommand,
    resume_attempt_id: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    let runnable = matches!(context.run_status.as_str(), "running" | "waiting");
    if context.run_status == "waiting"
        && connection
            .execute_raw(sql(
                "UPDATE graph_runs SET status = 'running', updated_at = ? WHERE id = ? AND status = 'waiting'",
                vec![now.into(), context.run_id.clone().into()],
            ))
            .await?
            .rows_affected()
            != 1
    {
        return Err(StorageError::Conflict("approval_run_status"));
    }
    let seq = append_event(
        connection,
        Event {
            run_id: &context.run_id,
            event_type: "llm.tool.approval_resolved",
            importance: "critical",
            node_instance_id: Some(&context.node_instance_id),
            attempt_id: Some(&context.node_attempt_id),
            payload: json!({
                "schemaVersion":1,
                "waitId":command.wait_id,
                "deliveryId":command.delivery_id,
                "resumeAttemptId":resume_attempt_id,
            }),
            now,
        },
    )
    .await?;
    if let Some(resume_attempt_id) = resume_attempt_id.filter(|_| runnable) {
        enqueue_wakeup(
            connection,
            &context.run_id,
            Some(&context.node_id),
            "attempt_ready",
            seq,
            &format!("wait-resume:{resume_attempt_id}"),
            now,
        )
        .await?;
    }
    Ok(())
}

async fn replay_delivery<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    payload_digest: &str,
) -> StorageResult<Option<WaitDeliveryView>> {
    let row = connection.query_one_raw(sql(
        "SELECT payload_digest, result_object_id FROM wait_deliveries WHERE wait_id = ? AND delivery_id = ?",
        vec![command.wait_id.clone().into(), command.delivery_id.clone().into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "payload_digest")? != payload_digest {
        return Err(StorageError::IdempotencyConflict);
    }
    let mut view: WaitDeliveryView =
        load_object_json(connection, &row.try_get::<String>("", "result_object_id")?).await?;
    view.replayed = true;
    Ok(Some(view))
}

fn validate_command(command: &SubmitWaitResponseCommand) -> StorageResult<()> {
    if [&command.wait_id, &command.delivery_id, &command.actor_kind]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256)
        || command
            .actor_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
        || !matches!(command.actor_kind.as_str(), "human" | "coordinator")
    {
        return Err(StorageError::InvalidArgument(
            "wait response is outside supported bounds".into(),
        ));
    }
    Ok(())
}
