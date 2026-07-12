use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::{EffectClassification, ToolFailureAction},
    llm::{EffectRetryPolicy, PrepareToolApprovalBatchCommand, PreparedToolApprovalBatch},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    runtime::{Event, add_object_ref, append_event},
};

use super::{
    model_ledger_helpers::{add_ref, persist_checkpoint},
    tool_approval_validation::{prepare_digest, to_i64, validate_batch_fields, validate_new_batch},
    tool_validation::validate_approval_tool_material,
    validation::load_ledger_context,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolApprovalContinuation {
    pub schema_version: u32,
    pub prepare_digest: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub model_call_id: String,
    pub checkpoint_ref: String,
    pub checkpoint_digest: String,
    pub calls: Vec<ToolApprovalCallPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolApprovalCallPlan {
    pub tool_call_id: String,
    pub effect_id: String,
    pub effect_attempt_id: String,
    pub call_index: u64,
    pub call_digest: String,
    pub arguments_ref: String,
    pub classification: EffectClassification,
    pub operation_key: String,
    pub idempotency_key: String,
    pub retry_policy: EffectRetryPolicy,
    pub requires_approval: bool,
    pub risk_summary: String,
    pub approval_expires_at: i64,
    pub denied_action: ToolFailureAction,
}

impl SqliteStore {
    pub async fn prepare_tool_approval_batch(
        &self,
        command: PrepareToolApprovalBatchCommand,
        now: i64,
    ) -> StorageResult<PreparedToolApprovalBatch> {
        validate_batch_fields(&command, now)?;
        let transaction = self.db.begin().await?;
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        let mut validated = Vec::with_capacity(command.calls.len());
        for call in &command.calls {
            validated.push(validate_approval_tool_material(
                &context,
                &command.checkpoint,
                call,
            )?);
        }
        let digest = prepare_digest(&command)?;
        if let Some(replay) = replay_batch(&transaction, &command, &digest).await? {
            transaction.commit().await?;
            return Ok(replay);
        }
        validate_new_batch(&transaction, &context, &command, &validated, now).await?;
        let mut plans = Vec::with_capacity(command.calls.len());
        for (call, material) in command.calls.iter().zip(&validated) {
            let arguments_ref =
                put_inline_object(&transaction, &canonical::to_vec(&material.arguments)?, now)
                    .await?;
            let status = if material.requires_approval {
                "awaiting_approval"
            } else {
                "validated"
            };
            transaction
                .execute_raw(sql(
                    "INSERT INTO tool_calls (id, node_instance_id, originating_attempt_id, model_call_id, provider_call_id, call_index, binding_id, tool_id, tool_version, call_digest, arguments_object_id, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    vec![
                        call.tool_call_id.clone().into(),
                        command.node_instance_id.clone().into(),
                        command.originating_attempt_id.clone().into(),
                        command.model_call_id.clone().into(),
                        call.provider_call_id.clone().into(),
                        to_i64(call.call_index, "tool call index")?.into(),
                        call.binding_id.clone().into(),
                        call.tool_id.clone().into(),
                        call.tool_version.clone().into(),
                        call.call_digest.clone().into(),
                        arguments_ref.clone().into(),
                        status.into(),
                        now.into(),
                    ],
                ))
                .await?;
            add_ref(
                &transaction,
                &arguments_ref,
                "tool_call",
                &call.tool_call_id,
                "arguments",
                now,
            )
            .await?;
            plans.push(ToolApprovalCallPlan {
                tool_call_id: call.tool_call_id.clone(),
                effect_id: call.effect_id.clone(),
                effect_attempt_id: call.effect_attempt_id.clone(),
                call_index: call.call_index,
                call_digest: call.call_digest.clone(),
                arguments_ref,
                classification: call.effect_classification,
                operation_key: call.effect_operation_key.clone(),
                idempotency_key: call.effect_idempotency_key.clone(),
                retry_policy: call.retry_policy.clone(),
                requires_approval: material.requires_approval,
                risk_summary: call.risk_summary.clone(),
                approval_expires_at: call.approval_expires_at,
                denied_action: material
                    .grant
                    .failure_policy
                    .as_ref()
                    .map_or(ToolFailureAction::ModelVisibleError, |policy| policy.denied),
            });
        }
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        open_approval_wait(&transaction, &command, &digest, plans, now).await?;
        transaction.commit().await?;
        Ok(PreparedToolApprovalBatch {
            wait_id: command.wait_id,
            tool_call_ids: command
                .calls
                .into_iter()
                .map(|call| call.tool_call_id)
                .collect(),
            replayed: false,
        })
    }
}

async fn open_approval_wait<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolApprovalBatchCommand,
    digest: &str,
    plans: Vec<ToolApprovalCallPlan>,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ni.run_id, ni.node_id, cp.checkpoint_object_id FROM node_instances ni JOIN llm_loop_checkpoints cp ON cp.node_instance_id = ni.id WHERE ni.id = ?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("approval wait owner is unavailable".into()))?;
    let run_id: String = row.try_get("", "run_id")?;
    let node_id: String = row.try_get("", "node_id")?;
    let checkpoint_ref: String = row.try_get("", "checkpoint_object_id")?;
    let approval_calls: Vec<_> = plans
        .iter()
        .filter(|call| call.requires_approval)
        .cloned()
        .collect();
    let deadline_at = approval_calls
        .iter()
        .map(|call| call.approval_expires_at)
        .min()
        .ok_or_else(|| StorageError::Integrity("approval blocker is missing".into()))?;
    let request = canonical::to_vec(&json!({
        "schemaVersion": 1,
        "kind": "tool_approval",
        "modelCallId": command.model_call_id,
        "calls": approval_calls.iter().map(|call| json!({
            "toolCallId": call.tool_call_id,
            "callDigest": call.call_digest,
            "riskSummary": call.risk_summary,
            "expiresAt": call.approval_expires_at,
        })).collect::<Vec<_>>(),
    }))?;
    let request_ref = put_inline_object(connection, &request, now).await?;
    let continuation = ToolApprovalContinuation {
        schema_version: 1,
        prepare_digest: digest.into(),
        node_instance_id: command.node_instance_id.clone(),
        originating_attempt_id: command.originating_attempt_id.clone(),
        model_call_id: command.model_call_id.clone(),
        checkpoint_ref: checkpoint_ref.clone(),
        checkpoint_digest: command.checkpoint.checksum.clone(),
        calls: plans,
    };
    let continuation_ref =
        put_inline_object(connection, &canonical::to_vec(&continuation)?, now).await?;
    connection
        .execute_raw(sql(
            "INSERT INTO node_waits (id, run_id, node_instance_id, node_attempt_id, kind, correlation_key, request_object_id, continuation_object_id, deadline_at, on_timeout, status, created_at) VALUES (?, ?, ?, ?, 'approval', ?, ?, ?, ?, 'fail', 'open', ?)",
            vec![
                command.wait_id.clone().into(),
                run_id.clone().into(),
                command.node_instance_id.clone().into(),
                command.originating_attempt_id.clone().into(),
                format!("tool-approval:{}", command.model_call_id).into(),
                request_ref.clone().into(),
                continuation_ref.clone().into(),
                deadline_at.into(),
                now.into(),
            ],
        ))
        .await?;
    for call in &approval_calls {
        connection
            .execute_raw(sql(
                "INSERT INTO wait_blockers (wait_id, blocker_kind, blocker_id, blocker_order, status) VALUES (?, 'tool_call', ?, ?, 'open')",
                vec![
                    command.wait_id.clone().into(),
                    call.tool_call_id.clone().into(),
                    to_i64(call.call_index, "tool blocker order")?.into(),
                ],
            ))
            .await?;
    }
    transition_owner_to_waiting(
        connection,
        command,
        &run_id,
        &node_id,
        &continuation_ref,
        now,
    )
    .await?;
    for (object_id, role) in [
        (&request_ref, "request"),
        (&continuation_ref, "continuation"),
        (&checkpoint_ref, "checkpoint"),
    ] {
        add_object_ref(
            connection,
            object_id,
            "node_wait",
            &command.wait_id,
            role,
            now,
        )
        .await?;
    }
    append_event(
        connection,
        Event {
            run_id: &run_id,
            event_type: "llm.tool.approval_requested",
            importance: "critical",
            node_instance_id: Some(&command.node_instance_id),
            attempt_id: Some(&command.originating_attempt_id),
            payload: json!({
                "schemaVersion": 1,
                "waitId": command.wait_id,
                "modelCallId": command.model_call_id,
                "toolCallIds": approval_calls.iter().map(|call| &call.tool_call_id).collect::<Vec<_>>(),
            }),
            now,
        },
    )
    .await?;
    Ok(())
}

async fn transition_owner_to_waiting<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolApprovalBatchCommand,
    run_id: &str,
    node_id: &str,
    continuation_ref: &str,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute_raw(sql(
        "UPDATE node_attempts SET status = 'waiting', continuation_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND node_instance_id = ? AND status = 'running'",
        vec![continuation_ref.into(), now.into(), command.originating_attempt_id.clone().into(), command.node_instance_id.clone().into()],
    )).await?;
    let instance = connection.execute_raw(sql(
        "UPDATE node_instances SET status = 'waiting', updated_at = ? WHERE id = ? AND status = 'running'",
        vec![now.into(), command.node_instance_id.clone().into()],
    )).await?;
    if attempt.rows_affected() != 1 || instance.rows_affected() != 1 {
        return Err(StorageError::Conflict("approval_wait_owner_status"));
    }
    connection.execute_raw(sql(
        "UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'",
        vec![command.originating_attempt_id.clone().into()],
    )).await?;
    connection.execute_raw(sql(
        "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE run_id = ? AND node_id = ? AND kind = 'attempt_ready' AND status = 'claimed'",
        vec![run_id.into(), node_id.into()],
    )).await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET open_waits = open_waits + 1 WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await?;
    let has_active_instance = connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM node_instances WHERE run_id = ? AND status IN ('ready','running') LIMIT 1",
            vec![run_id.into()],
        ))
        .await?
        .is_some();
    let has_dispatch_wakeup = connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM scheduler_wakeups WHERE run_id = ? AND kind IN ('node_maybe_ready','attempt_ready') AND status IN ('pending','claimed') LIMIT 1",
            vec![run_id.into()],
        ))
        .await?
        .is_some();
    if !has_active_instance && !has_dispatch_wakeup {
        let updated = connection
            .execute_raw(sql(
                "UPDATE graph_runs SET status = 'waiting', updated_at = ? WHERE id = ? AND status = 'running'",
                vec![now.into(), run_id.into()],
            ))
            .await?;
        if updated.rows_affected() == 1 {
            append_event(
                connection,
                Event {
                    run_id,
                    event_type: "run.waiting",
                    importance: "critical",
                    node_instance_id: None,
                    attempt_id: None,
                    payload: json!({"schemaVersion":1,"reason":"tool_approval"}),
                    now,
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn replay_batch<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareToolApprovalBatchCommand,
    digest: &str,
) -> StorageResult<Option<PreparedToolApprovalBatch>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT kind, node_instance_id, continuation_object_id FROM node_waits WHERE id = ?",
            vec![command.wait_id.clone().into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "kind")? != "approval"
        || row.try_get::<String>("", "node_instance_id")? != command.node_instance_id
    {
        return Err(StorageError::Conflict("tool_approval_batch_replay"));
    }
    let continuation: ToolApprovalContinuation = load_object_json(
        connection,
        &row.try_get::<String>("", "continuation_object_id")?,
    )
    .await?;
    let tool_call_ids: Vec<_> = command
        .calls
        .iter()
        .map(|call| call.tool_call_id.clone())
        .collect();
    if continuation.schema_version != 1
        || continuation.prepare_digest != digest
        || continuation.checkpoint_digest != command.checkpoint.checksum
        || continuation.model_call_id != command.model_call_id
        || continuation
            .calls
            .iter()
            .map(|call| call.tool_call_id.clone())
            .collect::<Vec<_>>()
            != tool_call_ids
    {
        return Err(StorageError::Conflict("tool_approval_batch_replay"));
    }
    Ok(Some(PreparedToolApprovalBatch {
        wait_id: command.wait_id.clone(),
        tool_call_ids,
        replayed: true,
    }))
}
