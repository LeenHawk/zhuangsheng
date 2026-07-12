use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectResolutionKind, EffectResolutionView, EffectRetryPolicy, LlmLogicalCallStatus,
        LlmLoopCheckpoint, ResolveEffectUnknownCommand, ToolCallCheckpointStatus,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_bytes, load_object_json, put_inline_object, sql},
    runtime::add_object_ref,
};

use super::{
    effect_resolution_helpers::{
        ResolutionContext, ResolutionOwner, actor_kind_name, command_digest, ensure_live_object,
        load_resolution_context, replay_resolution, resolution_kind_name, validate_command,
    },
    effect_resolution_settle::settle_blocker,
    model_ledger_helpers::persist_checkpoint,
    tool_ledger_finish::validate_tool_output,
};

impl SqliteStore {
    pub async fn resolve_effect_unknown(
        &self,
        command: ResolveEffectUnknownCommand,
        now: i64,
    ) -> StorageResult<EffectResolutionView> {
        validate_command(&command)?;
        let digest = command_digest(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(replayed) = replay_resolution(&transaction, &command, &digest).await? {
            transaction.commit().await?;
            return Ok(replayed);
        }
        if transaction
            .query_one(sql(
                "SELECT 1 AS present FROM effect_resolutions WHERE effect_attempt_id = ?",
                vec![command.expected_effect_attempt_id.clone().into()],
            ))
            .await?
            .is_some()
        {
            return Err(StorageError::Conflict("effect_already_resolved"));
        }
        let context = load_resolution_context(&transaction, &command).await?;
        validate_resolution_material(&transaction, &command, &context).await?;
        let decision_object_id =
            put_inline_object(&transaction, &canonical::to_vec(&command.decision)?, now).await?;
        transaction
            .execute(sql(
                "INSERT INTO effect_resolutions (id, effect_id, effect_attempt_id, resolution_kind, command_idempotency_key, request_digest, decision_object_id, result_object_id, evidence_object_id, actor_kind, actor_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![
                    command.resolution_id.clone().into(),
                    command.effect_id.clone().into(),
                    command.expected_effect_attempt_id.clone().into(),
                    resolution_kind_name(command.kind).into(),
                    command.command_idempotency_key.clone().into(),
                    digest.into(),
                    decision_object_id.clone().into(),
                    command.result_object_id.clone().into(),
                    command.evidence_object_id.clone().into(),
                    actor_kind_name(command.actor_kind).into(),
                    command.actor_id.clone().into(),
                    now.into(),
                ],
            ))
            .await?;
        apply_projection(&transaction, &command, &context, now).await?;
        settle_blocker(&transaction, &command, &context, &decision_object_id, now).await?;
        add_resolution_refs(&transaction, &command, &context, &decision_object_id, now).await?;
        let view = EffectResolutionView {
            resolution_id: command.resolution_id,
            effect_id: command.effect_id,
            effect_attempt_id: command.expected_effect_attempt_id,
            wait_id: context.wait_id,
            kind: command.kind,
            replayed: false,
        };
        transaction.commit().await?;
        Ok(view)
    }
}

async fn validate_resolution_material<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
) -> StorageResult<()> {
    if let Some(object_id) = &command.result_object_id {
        ensure_live_object(connection, object_id).await?;
        if matches!(&context.owner, ResolutionOwner::Tool(_)) {
            validate_tool_output(&load_object_bytes(connection, object_id).await?)?;
        }
    }
    if let Some(object_id) = &command.evidence_object_id {
        ensure_live_object(connection, object_id).await?;
    }
    if command.kind == EffectResolutionKind::ConfirmFailedRetrySafe {
        let policy: EffectRetryPolicy = serde_json::from_str(&context.retry_policy_json)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
        if context.attempt_no >= policy.max_attempts {
            return Err(StorageError::InvalidArgument(
                "effect retry is not allowed by the pinned attempt budget".into(),
            ));
        }
        if context.classification == "non_idempotent" && command.evidence_object_id.is_none() {
            return Err(StorageError::InvalidArgument(
                "non-idempotent retry-safe resolution requires evidence".into(),
            ));
        }
    }
    Ok(())
}

async fn apply_projection<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    now: i64,
) -> StorageResult<()> {
    let (effect_status, model_status, completed_at) = match command.kind {
        EffectResolutionKind::ConfirmSucceeded => ("succeeded", "completed", Some(now)),
        EffectResolutionKind::ConfirmFailedRetrySafe => ("pending", "retry_ready", None),
        EffectResolutionKind::AbortRun => ("abandoned_unknown", "abandoned_unknown", Some(now)),
    };
    let effect = connection
        .execute(sql(
            "UPDATE effects SET status = ?, result_object_id = ?, completed_at = ? WHERE id = ? AND status = 'outcome_unknown'",
            vec![
                effect_status.into(),
                command.result_object_id.clone().into(),
                completed_at.into(),
                command.effect_id.clone().into(),
            ],
        ))
        .await?;
    let owner = match &context.owner {
        ResolutionOwner::Model(id) => {
            connection
                .execute(sql(
                    "UPDATE model_calls SET status = ?, response_object_id = ? WHERE id = ? AND status = 'outcome_unknown'",
                    vec![
                        model_status.into(),
                        command.result_object_id.clone().into(),
                        id.clone().into(),
                    ],
                ))
                .await?
        }
        ResolutionOwner::Tool(id) => {
            connection
                .execute(sql(
                    "UPDATE tool_calls SET status = ?, output_object_id = ? WHERE id = ? AND status = 'outcome_unknown'",
                    vec![
                        model_status.into(),
                        command.result_object_id.clone().into(),
                        id.clone().into(),
                    ],
                ))
                .await?
        }
    };
    if effect.rows_affected() != 1 || owner.rows_affected() != 1 {
        return Err(StorageError::Conflict("effect_resolution_projection"));
    }
    update_checkpoint(connection, command, context, now).await
}

async fn update_checkpoint<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![context.node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("effect checkpoint missing".into()))?;
    let object_id: String = row.try_get("", "checkpoint_object_id")?;
    let mut checkpoint: LlmLoopCheckpoint = load_object_json(connection, &object_id).await?;
    if !checkpoint.checksum_is_valid() {
        return Err(StorageError::Integrity(
            "effect checkpoint checksum is invalid".into(),
        ));
    }
    if !checkpoint.wait_ids.contains(&context.wait_id) {
        return Err(StorageError::Integrity(
            "effect checkpoint does not match unknown projection".into(),
        ));
    }
    match &context.owner {
        ResolutionOwner::Model(model_call_id) => {
            let active = checkpoint.active_model_effect.as_mut().ok_or_else(|| {
                StorageError::Integrity("active model effect missing during resolution".into())
            })?;
            if active.model_call_id != model_call_id.as_str()
                || active.effect_id != command.effect_id
                || active.status != LlmLogicalCallStatus::OutcomeUnknown
            {
                return Err(StorageError::Integrity(
                    "model effect checkpoint does not match unknown projection".into(),
                ));
            }
            match command.kind {
                EffectResolutionKind::ConfirmSucceeded => {
                    active.status = LlmLogicalCallStatus::Completed;
                    active.response_ref = command.result_object_id.clone();
                }
                EffectResolutionKind::ConfirmFailedRetrySafe => {
                    active.status = LlmLogicalCallStatus::RetryReady;
                    active.response_ref = None;
                }
                EffectResolutionKind::AbortRun => {
                    active.status = LlmLogicalCallStatus::AbandonedUnknown;
                    active.response_ref = None;
                }
            }
        }
        ResolutionOwner::Tool(tool_call_id) => {
            let active = checkpoint
                .current_batch
                .iter_mut()
                .find(|call| {
                    call.tool_call_id == tool_call_id.as_str()
                        && call.effect_id.as_deref() == Some(command.effect_id.as_str())
                })
                .ok_or_else(|| {
                    StorageError::Integrity("tool effect missing during resolution".into())
                })?;
            if active.status != ToolCallCheckpointStatus::OutcomeUnknown {
                return Err(StorageError::Integrity(
                    "tool effect checkpoint does not match unknown projection".into(),
                ));
            }
            match command.kind {
                EffectResolutionKind::ConfirmSucceeded => {
                    active.status = ToolCallCheckpointStatus::Completed;
                    active.output_ref = command.result_object_id.clone();
                }
                EffectResolutionKind::ConfirmFailedRetrySafe => {
                    active.status = ToolCallCheckpointStatus::RetryReady;
                    active.output_ref = None;
                }
                EffectResolutionKind::AbortRun => {
                    active.status = ToolCallCheckpointStatus::AbandonedUnknown;
                    active.output_ref = None;
                }
            }
        }
    }
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await
}

async fn add_resolution_refs<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    decision_object_id: &str,
    now: i64,
) -> StorageResult<()> {
    add_object_ref(
        connection,
        decision_object_id,
        "effect_resolution",
        &command.resolution_id,
        "decision",
        now,
    )
    .await?;
    if let Some(object_id) = &command.result_object_id {
        let (owner_kind, owner_role) = match &context.owner {
            ResolutionOwner::Model(_) => ("model_call", "response"),
            ResolutionOwner::Tool(_) => ("tool_call", "result"),
        };
        for (owner_kind, owner_id, role) in [
            (
                "effect_resolution",
                command.resolution_id.as_str(),
                "result",
            ),
            ("effect", command.effect_id.as_str(), "result"),
            (owner_kind, context.owner.id(), owner_role),
        ] {
            add_object_ref(connection, object_id, owner_kind, owner_id, role, now).await?;
        }
    }
    if let Some(object_id) = &command.evidence_object_id {
        add_object_ref(
            connection,
            object_id,
            "effect_resolution",
            &command.resolution_id,
            "evidence",
            now,
        )
        .await?;
    }
    Ok(())
}
