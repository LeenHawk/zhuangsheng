use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    graph::LlmNodeExecutionSnapshot,
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, LlmLogicalCallStatus, LlmLoopCheckpoint,
        LlmOperationExecutionPin,
    },
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

pub(super) struct LedgerContext {
    pub graph_revision_id: String,
    pub execution_snapshot_object_id: String,
    pub snapshot: LlmNodeExecutionSnapshot,
}

pub(super) async fn load_ledger_context<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
    node_attempt_id: &str,
) -> StorageResult<LedgerContext> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ni.graph_revision_id, ni.execution_snapshot_object_id, a.node_instance_id AS attempt_instance_id FROM node_instances ni JOIN node_attempts a ON a.id = ? WHERE ni.id = ?",
            vec![node_attempt_id.into(), node_instance_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "node_instance_or_attempt",
            id: format!("{node_instance_id}:{node_attempt_id}"),
        })?;
    let attempt_instance_id: String = row.try_get("", "attempt_instance_id")?;
    if attempt_instance_id != node_instance_id {
        return Err(StorageError::InvalidArgument(
            "effect attempt owner does not belong to node instance".into(),
        ));
    }
    let graph_revision_id: String = row.try_get("", "graph_revision_id")?;
    let execution_snapshot_object_id: Option<String> =
        row.try_get("", "execution_snapshot_object_id")?;
    let execution_snapshot_object_id = execution_snapshot_object_id.ok_or_else(|| {
        StorageError::InvalidArgument("LLM execution snapshot is not pinned".into())
    })?;
    let snapshot: LlmNodeExecutionSnapshot =
        load_object_json(connection, &execution_snapshot_object_id).await?;
    if snapshot.graph_revision_id != graph_revision_id {
        return Err(StorageError::Integrity(
            "LLM snapshot graph revision mismatch".into(),
        ));
    }
    Ok(LedgerContext {
        graph_revision_id,
        execution_snapshot_object_id,
        snapshot,
    })
}

pub(super) fn validate_operation(
    expected: &LlmOperationExecutionPin,
    actual: &LlmOperationExecutionPin,
    channel_id: &str,
    context: &LedgerContext,
) -> StorageResult<()> {
    if expected != actual
        || context.snapshot.operation != *actual
        || context.snapshot.channel.id != actual.channel_revision_id
        || context.snapshot.channel.channel_id != channel_id
    {
        return Err(StorageError::InvalidArgument(
            "model call operation does not match execution snapshot".into(),
        ));
    }
    Ok(())
}

pub(super) struct CheckpointExpectation<'a> {
    pub context: &'a LedgerContext,
    pub node_instance_id: &'a str,
    pub updater_attempt_id: &'a str,
    pub call_no: u64,
    pub model_call_id: &'a str,
    pub effect_id: &'a str,
    pub effect_attempt_id: &'a str,
    pub status: LlmLogicalCallStatus,
    pub response_ref: Option<&'a str>,
}

pub(super) fn validate_checkpoint(
    checkpoint: &LlmLoopCheckpoint,
    expected: CheckpointExpectation<'_>,
) -> StorageResult<()> {
    let expected_active = ActiveModelEffectCheckpoint {
        model_call_id: expected.model_call_id.into(),
        effect_id: expected.effect_id.into(),
        status: expected.status,
        response_ref: expected.response_ref.map(str::to_owned),
    };
    let max_model_calls = expected
        .context
        .snapshot
        .limits
        .max_model_calls
        .ok_or_else(|| StorageError::Integrity("LLM model-call limit is not pinned".into()))?;
    if checkpoint.schema_version != 1
        || !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != expected.node_instance_id
        || checkpoint.last_updated_by_attempt_id != expected.updater_attempt_id
        || checkpoint.graph_revision_id != expected.context.graph_revision_id
        || checkpoint.context_snapshot_ref != expected.context.execution_snapshot_object_id
        || checkpoint.model_call_no != expected.call_no
        || checkpoint.model_calls_used != expected.call_no
        || checkpoint.model_calls_used > max_model_calls
        || checkpoint.effect_watermark != expected.effect_attempt_id
        || checkpoint.active_model_effect.as_ref() != Some(&expected_active)
        || checkpoint.registry_snapshot.revision.is_empty()
        || checkpoint.registry_snapshot != expected.context.snapshot.tool_registry
        || checkpoint.read_set_digest.is_empty()
        || checkpoint.transcript_ref.is_empty()
    {
        return Err(StorageError::InvalidArgument(
            "LLM loop checkpoint is incompatible with ledger transition".into(),
        ));
    }
    Ok(())
}

pub(super) async fn validate_fence<C: ConnectionTrait>(
    connection: &C,
    effect_attempt_id: &str,
    fence: &EffectAttemptFence,
) -> StorageResult<FencedModelCall> {
    let fenced = load_model_call_attempt(connection, effect_attempt_id, fence).await?;
    validate_active_fence(&fenced, fence)?;
    Ok(fenced)
}

pub(super) async fn load_model_call_attempt<C: ConnectionTrait>(
    connection: &C,
    effect_attempt_id: &str,
    fence: &EffectAttemptFence,
) -> StorageResult<FencedModelCall> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ea.effect_id, ea.status AS attempt_status, ea.provider_request_id AS attempt_provider_request_id, ea.result_object_id AS attempt_result_object_id, attempt_result.content_hash AS attempt_result_digest, attempt_error.content_hash AS attempt_error_digest, e.model_call_id, e.node_instance_id, e.status AS effect_status, e.classification, mc.call_no, mc.status AS model_status, mc.provider_request_id AS model_provider_request_id, mc.response_object_id, model_response.content_hash AS response_digest, mc.usage_json, cp.checkpoint_digest, a.status AS node_attempt_status, a.worker_id, a.lease_fence, a.run_control_epoch, r.status AS run_status, r.control_epoch FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id JOIN model_calls mc ON mc.id = e.model_call_id JOIN node_attempts a ON a.id = ea.invoking_node_attempt_id JOIN node_instances ni ON ni.id = e.node_instance_id JOIN graph_runs r ON r.id = ni.run_id LEFT JOIN content_objects attempt_result ON attempt_result.id = ea.result_object_id LEFT JOIN content_objects attempt_error ON attempt_error.id = ea.error_object_id LEFT JOIN content_objects model_response ON model_response.id = mc.response_object_id LEFT JOIN llm_loop_checkpoints cp ON cp.node_instance_id = e.node_instance_id WHERE ea.id = ? AND ea.invoking_node_attempt_id = ?",
            vec![effect_attempt_id.into(), fence.invoking_node_attempt_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "effect_attempt",
            id: effect_attempt_id.into(),
        })?;
    Ok(FencedModelCall {
        effect_id: row.try_get("", "effect_id")?,
        model_call_id: row.try_get("", "model_call_id")?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        call_no: u64::try_from(row.try_get::<i64>("", "call_no")?)
            .map_err(|_| StorageError::Integrity("invalid model call number".into()))?,
        attempt_status: row.try_get("", "attempt_status")?,
        effect_status: row.try_get("", "effect_status")?,
        model_status: row.try_get("", "model_status")?,
        classification: row.try_get("", "classification")?,
        attempt_provider_request_id: row.try_get("", "attempt_provider_request_id")?,
        model_provider_request_id: row.try_get("", "model_provider_request_id")?,
        attempt_result_object_id: row.try_get("", "attempt_result_object_id")?,
        attempt_result_digest: row.try_get("", "attempt_result_digest")?,
        attempt_error_digest: row.try_get("", "attempt_error_digest")?,
        response_object_id: row.try_get("", "response_object_id")?,
        response_digest: row.try_get("", "response_digest")?,
        usage_json: row.try_get("", "usage_json")?,
        checkpoint_digest: row.try_get("", "checkpoint_digest")?,
        node_attempt_status: row.try_get("", "node_attempt_status")?,
        worker_id: row.try_get("", "worker_id")?,
        lease_fence: row.try_get("", "lease_fence")?,
        attempt_epoch: row.try_get("", "run_control_epoch")?,
        run_status: row.try_get("", "run_status")?,
        control_epoch: row.try_get("", "control_epoch")?,
    })
}

pub(super) fn validate_active_fence(
    fenced: &FencedModelCall,
    fence: &EffectAttemptFence,
) -> StorageResult<()> {
    if fenced.worker_id.as_deref() != Some(&fence.worker_id)
        || u64::try_from(fenced.lease_fence).ok() != Some(fence.lease_fence)
        || u64::try_from(fenced.attempt_epoch).ok() != Some(fence.run_control_epoch)
        || fenced.attempt_epoch != fenced.control_epoch
        || fenced.node_attempt_status != "running"
        || fenced.run_status != "running"
    {
        return Err(StorageError::Conflict("effect_attempt_fence"));
    }
    Ok(())
}

pub(super) fn validate_replay_fence(
    fenced: &FencedModelCall,
    fence: &EffectAttemptFence,
) -> StorageResult<()> {
    if fenced
        .worker_id
        .as_deref()
        .is_some_and(|worker| worker != fence.worker_id)
        || u64::try_from(fenced.lease_fence).ok() != Some(fence.lease_fence)
        || u64::try_from(fenced.attempt_epoch).ok() != Some(fence.run_control_epoch)
    {
        return Err(StorageError::Conflict("effect_attempt_fence"));
    }
    Ok(())
}

pub(super) struct FencedModelCall {
    pub effect_id: String,
    pub model_call_id: String,
    pub node_instance_id: String,
    pub call_no: u64,
    pub attempt_status: String,
    pub effect_status: String,
    pub model_status: String,
    pub classification: String,
    pub attempt_provider_request_id: Option<String>,
    pub model_provider_request_id: Option<String>,
    pub attempt_result_object_id: Option<String>,
    pub attempt_result_digest: Option<String>,
    pub attempt_error_digest: Option<String>,
    pub response_object_id: Option<String>,
    pub response_digest: Option<String>,
    pub usage_json: Option<String>,
    pub checkpoint_digest: Option<String>,
    pub node_attempt_status: String,
    pub worker_id: Option<String>,
    pub lease_fence: i64,
    pub attempt_epoch: i64,
    pub run_status: String,
    pub control_epoch: i64,
}

pub(super) async fn validate_node_attempt_fence<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
    fence: &EffectAttemptFence,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT a.node_instance_id, a.status, a.worker_id, a.lease_fence, a.run_control_epoch, r.status AS run_status, r.control_epoch FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE a.id = ?",
            vec![fence.invoking_node_attempt_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "node_attempt",
            id: fence.invoking_node_attempt_id.clone(),
        })?;
    let attempt_instance: String = row.try_get("", "node_instance_id")?;
    let status: String = row.try_get("", "status")?;
    let worker: Option<String> = row.try_get("", "worker_id")?;
    let lease: i64 = row.try_get("", "lease_fence")?;
    let attempt_epoch: i64 = row.try_get("", "run_control_epoch")?;
    let run_status: String = row.try_get("", "run_status")?;
    let control_epoch: i64 = row.try_get("", "control_epoch")?;
    if attempt_instance != node_instance_id
        || status != "running"
        || worker.as_deref() != Some(&fence.worker_id)
        || u64::try_from(lease).ok() != Some(fence.lease_fence)
        || u64::try_from(attempt_epoch).ok() != Some(fence.run_control_epoch)
        || attempt_epoch != control_epoch
        || run_status != "running"
    {
        return Err(StorageError::Conflict("effect_attempt_fence"));
    }
    Ok(())
}
