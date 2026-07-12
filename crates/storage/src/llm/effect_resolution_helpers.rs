use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectResolutionActorKind, EffectResolutionKind, EffectResolutionView,
        ResolveEffectUnknownCommand,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) struct ResolutionContext {
    pub node_instance_id: String,
    pub owner: ResolutionOwner,
    pub classification: String,
    pub retry_policy_json: String,
    pub attempt_no: u32,
    pub invoking_node_attempt_id: String,
    pub run_id: String,
    pub node_id: String,
    pub run_status: String,
    pub control_epoch: u64,
    pub wait_id: String,
    pub wait_attempt_id: String,
}

pub(super) enum ResolutionOwner {
    Model(String),
    Tool(String),
}

impl ResolutionOwner {
    pub fn id(&self) -> &str {
        match self {
            Self::Model(id) | Self::Tool(id) => id,
        }
    }
}

pub(super) fn validate_command(command: &ResolveEffectUnknownCommand) -> StorageResult<()> {
    let ids = [
        &command.resolution_id,
        &command.effect_id,
        &command.expected_effect_attempt_id,
        &command.command_idempotency_key,
    ];
    let actor_valid = command
        .actor_id
        .as_ref()
        .is_some_and(|actor| !actor.trim().is_empty() && actor.len() <= 256);
    let decision_size = canonical::to_vec(&command.decision)?.len();
    if ids
        .iter()
        .any(|value| value.trim().is_empty() || value.len() > 256)
        || !actor_valid
        || decision_size == 0
        || decision_size > 1024 * 1024
    {
        return Err(StorageError::InvalidArgument(
            "effect resolution command is outside supported bounds".into(),
        ));
    }
    match command.kind {
        EffectResolutionKind::ConfirmSucceeded if command.result_object_id.is_none() => Err(
            StorageError::InvalidArgument("confirm_succeeded requires a result object".into()),
        ),
        EffectResolutionKind::ConfirmFailedRetrySafe if command.result_object_id.is_some() => {
            Err(StorageError::InvalidArgument(
                "confirm_failed_retry_safe cannot carry a result object".into(),
            ))
        }
        EffectResolutionKind::AbortRun if command.result_object_id.is_some() => Err(
            StorageError::InvalidArgument("abort_run cannot carry a result object".into()),
        ),
        _ => Ok(()),
    }
}

pub(super) fn command_digest(command: &ResolveEffectUnknownCommand) -> StorageResult<String> {
    Ok(canonical::hash(&json!({
        "resolutionId": command.resolution_id,
        "effectId": command.effect_id,
        "expectedEffectAttemptId": command.expected_effect_attempt_id,
        "expectedRunControlEpoch": command.expected_run_control_epoch,
        "kind": command.kind,
        "decision": command.decision,
        "resultObjectId": command.result_object_id,
        "evidenceObjectId": command.evidence_object_id,
        "actorKind": command.actor_kind,
        "actorId": command.actor_id,
    }))?)
}

pub(super) async fn replay_resolution<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    digest: &str,
) -> StorageResult<Option<EffectResolutionView>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT er.id, er.effect_attempt_id, er.resolution_kind, er.request_digest, wb.wait_id FROM effect_resolutions er LEFT JOIN wait_blockers wb ON wb.blocker_kind = 'effect' AND wb.blocker_id = er.effect_id WHERE er.effect_id = ? AND er.command_idempotency_key = ?",
            vec![
                command.effect_id.clone().into(),
                command.command_idempotency_key.clone().into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    Ok(Some(EffectResolutionView {
        resolution_id: row.try_get("", "id")?,
        effect_id: command.effect_id.clone(),
        effect_attempt_id: row.try_get("", "effect_attempt_id")?,
        wait_id: row.try_get("", "wait_id")?,
        kind: parse_resolution_kind(&row.try_get::<String>("", "resolution_kind")?)?,
        replayed: true,
    }))
}

pub(super) async fn load_resolution_context<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
) -> StorageResult<ResolutionContext> {
    let row = connection
        .query_one_raw(sql(
            "SELECT e.node_instance_id, e.model_call_id, e.count_call_id, e.tool_call_id, e.status AS effect_status, e.classification, e.retry_policy_json, ea.status AS attempt_status, ea.attempt_no, ea.invoking_node_attempt_id, mc.status AS model_status, tc.status AS tool_status, ni.run_id, ni.node_id, r.status AS run_status, r.control_epoch, w.id AS wait_id, w.node_attempt_id AS wait_attempt_id, w.status AS wait_status, wb.status AS blocker_status FROM effects e JOIN effect_attempts ea ON ea.effect_id = e.id AND ea.id = ? LEFT JOIN model_calls mc ON mc.id = e.model_call_id LEFT JOIN tool_calls tc ON tc.id = e.tool_call_id JOIN node_instances ni ON ni.id = e.node_instance_id JOIN graph_runs r ON r.id = ni.run_id JOIN wait_blockers wb ON wb.blocker_kind = 'effect' AND wb.blocker_id = e.id JOIN node_waits w ON w.id = wb.wait_id AND w.node_instance_id = e.node_instance_id AND w.kind = 'effect_resolution' WHERE e.id = ?",
            vec![
                command.expected_effect_attempt_id.clone().into(),
                command.effect_id.clone().into(),
            ],
        ))
        .await?
        .ok_or_else(|| StorageError::Conflict("effect_resolution_wait"))?;
    let model_call_id: Option<String> = row.try_get("", "model_call_id")?;
    let count_call_id: Option<String> = row.try_get("", "count_call_id")?;
    let tool_call_id: Option<String> = row.try_get("", "tool_call_id")?;
    let owner = match (model_call_id, count_call_id, tool_call_id) {
        (Some(id), None, None) => ResolutionOwner::Model(id),
        (None, None, Some(id)) => ResolutionOwner::Tool(id),
        _ => {
            return Err(StorageError::InvalidArgument(
                "effect resolution requires a model or tool owner".into(),
            ));
        }
    };
    let owner_status: Option<String> = match &owner {
        ResolutionOwner::Model(_) => row.try_get("", "model_status")?,
        ResolutionOwner::Tool(_) => row.try_get("", "tool_status")?,
    };
    if owner_status.as_deref() != Some("outcome_unknown") {
        return Err(StorageError::InvalidArgument(
            "effect resolution owner is not outcome-unknown".into(),
        ));
    }
    if row.try_get::<String>("", "effect_status")? != "outcome_unknown"
        || row.try_get::<String>("", "attempt_status")? != "outcome_unknown"
        || row.try_get::<String>("", "wait_status")? != "open"
        || row.try_get::<String>("", "blocker_status")? != "open"
    {
        return Err(StorageError::Conflict("effect_resolution_status"));
    }
    let control_epoch = u64::try_from(row.try_get::<i64>("", "control_epoch")?)
        .map_err(|_| StorageError::Integrity("invalid run control epoch".into()))?;
    if control_epoch != command.expected_run_control_epoch {
        return Err(StorageError::Conflict("run_control_epoch"));
    }
    let run_status: String = row.try_get("", "run_status")?;
    if matches!(run_status.as_str(), "completed" | "failed" | "cancelled") {
        return Err(StorageError::Conflict("run_terminal"));
    }
    let invoking_node_attempt_id: String = row.try_get("", "invoking_node_attempt_id")?;
    let wait_attempt_id: String = row.try_get("", "wait_attempt_id")?;
    if invoking_node_attempt_id != wait_attempt_id {
        return Err(StorageError::Integrity(
            "effect wait is bound to a different node attempt".into(),
        ));
    }
    Ok(ResolutionContext {
        node_instance_id: row.try_get("", "node_instance_id")?,
        owner,
        classification: row.try_get("", "classification")?,
        retry_policy_json: row.try_get("", "retry_policy_json")?,
        attempt_no: u32::try_from(row.try_get::<i64>("", "attempt_no")?)
            .map_err(|_| StorageError::Integrity("invalid effect attempt number".into()))?,
        invoking_node_attempt_id,
        run_id: row.try_get("", "run_id")?,
        node_id: row.try_get("", "node_id")?,
        run_status,
        control_epoch,
        wait_id: row.try_get("", "wait_id")?,
        wait_attempt_id,
    })
}

pub(super) async fn ensure_live_object<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
) -> StorageResult<()> {
    if connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM content_objects WHERE id = ? AND lifecycle = 'live'",
            vec![object_id.into()],
        ))
        .await?
        .is_none()
    {
        return Err(StorageError::InvalidArgument(
            "effect resolution object is unavailable".into(),
        ));
    }
    Ok(())
}

pub(super) fn resolution_kind_name(kind: EffectResolutionKind) -> &'static str {
    match kind {
        EffectResolutionKind::ConfirmSucceeded => "confirm_succeeded",
        EffectResolutionKind::ConfirmFailedRetrySafe => "confirm_failed_retry_safe",
        EffectResolutionKind::AbortRun => "abort_run",
    }
}

pub(super) fn actor_kind_name(kind: EffectResolutionActorKind) -> &'static str {
    match kind {
        EffectResolutionActorKind::Human => "human",
        EffectResolutionActorKind::Coordinator => "coordinator",
    }
}

fn parse_resolution_kind(value: &str) -> StorageResult<EffectResolutionKind> {
    match value {
        "confirm_succeeded" => Ok(EffectResolutionKind::ConfirmSucceeded),
        "confirm_failed_retry_safe" => Ok(EffectResolutionKind::ConfirmFailedRetrySafe),
        "abort_run" => Ok(EffectResolutionKind::AbortRun),
        _ => Err(StorageError::Integrity(
            "unknown effect resolution kind".into(),
        )),
    }
}
