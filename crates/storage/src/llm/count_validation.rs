use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    graph::LlmNodeExecutionSnapshot,
    llm::{
        ActiveCountEffectCheckpoint, CountExecutionPin, CountResultSource, EffectAttemptFence,
        LlmLogicalCallStatus, LlmLoopCheckpoint, Operation, OperationGroup,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::validation::LedgerContext;

pub(super) fn validate_count_pin(
    snapshot: &LlmNodeExecutionSnapshot,
    pin: &CountExecutionPin,
    channel_id: &str,
) -> StorageResult<String> {
    let max_input_tokens = snapshot
        .limits
        .max_input_tokens
        .ok_or_else(|| StorageError::Integrity("count input-token limit is not pinned".into()))?;
    let expected_margin = max_input_tokens.div_ceil(20).max(256);
    let provider_valid = pin.provider_count_operation_key.is_none_or(|operation| {
        operation.operation == Operation::CountTokens
            && operation.group() == OperationGroup::CountTokens
            && operation.provider_family() == snapshot.operation.operation_key.provider_family()
            && snapshot.channel.spec.operation_keys.contains(&operation)
    });
    if snapshot.channel.channel_id != channel_id
        || pin.generation_operation != snapshot.operation
        || pin.local_counter_id != "gproxy_tokenize"
        || pin.local_counter_version != 1
        || pin.fallback_policy_version != 1
        || pin.safety_margin_tokens != expected_margin
        || !provider_valid
    {
        return Err(StorageError::InvalidArgument(
            "count execution pin does not match the node execution snapshot".into(),
        ));
    }
    Ok(pin.digest()?)
}

pub(super) struct CountCheckpointExpectation<'a> {
    pub context: &'a LedgerContext,
    pub node_instance_id: &'a str,
    pub updater_attempt_id: &'a str,
    pub count_call_id: &'a str,
    pub effect_id: &'a str,
    pub effect_attempt_id: &'a str,
    pub count_ordinal: u64,
    pub pin_digest: &'a str,
    pub candidate_ref: &'a str,
    pub candidate_digest: &'a str,
    pub request_digest: &'a str,
    pub status: LlmLogicalCallStatus,
    pub result_source: Option<CountResultSource>,
    pub result_ref: Option<&'a str>,
}

pub(super) fn validate_count_checkpoint(
    checkpoint: &LlmLoopCheckpoint,
    expected: CountCheckpointExpectation<'_>,
) -> StorageResult<()> {
    let active = ActiveCountEffectCheckpoint {
        count_call_id: expected.count_call_id.into(),
        effect_id: expected.effect_id.into(),
        count_ordinal: expected.count_ordinal,
        count_execution_pin_digest: expected.pin_digest.into(),
        trim_candidate_ref: expected.candidate_ref.into(),
        trim_candidate_digest: expected.candidate_digest.into(),
        request_digest: expected.request_digest.into(),
        status: expected.status,
        result_source: expected.result_source,
        result_ref: expected.result_ref.map(str::to_owned),
    };
    let max_count_calls = expected
        .context
        .snapshot
        .limits
        .max_count_calls
        .ok_or_else(|| StorageError::Integrity("count-call limit is not pinned".into()))?;
    if checkpoint.schema_version != 1
        || !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != expected.node_instance_id
        || checkpoint.last_updated_by_attempt_id != expected.updater_attempt_id
        || checkpoint.graph_revision_id != expected.context.graph_revision_id
        || checkpoint.context_snapshot_ref != expected.context.execution_snapshot_object_id
        || checkpoint.count_calls_used != expected.count_ordinal
        || checkpoint.count_calls_used > max_count_calls
        || checkpoint.effect_watermark != expected.effect_attempt_id
        || checkpoint.active_count_effect.as_ref() != Some(&active)
        || checkpoint.registry_snapshot.revision.is_empty()
        || checkpoint.read_set_digest.is_empty()
        || checkpoint.transcript_ref.is_empty()
    {
        return Err(StorageError::InvalidArgument(
            "LLM checkpoint is incompatible with count transition".into(),
        ));
    }
    Ok(())
}

pub(super) struct FencedCountCall {
    pub effect_id: String,
    pub count_call_id: String,
    pub node_instance_id: String,
    pub count_ordinal: u64,
    pub pin_digest: String,
    pub candidate_ref: String,
    pub candidate_digest: String,
    pub request_digest: String,
    pub attempt_status: String,
    pub effect_status: String,
    pub count_status: String,
    pub provider_count_available: bool,
    pub attempt_provider_request_id: Option<String>,
    pub result_object_id: Option<String>,
    pub result_digest: Option<String>,
    pub error_digest: Option<String>,
    pub result_source: Option<String>,
    pub checkpoint_digest: Option<String>,
    pub node_attempt_status: String,
    pub worker_id: Option<String>,
    pub lease_fence: i64,
    pub attempt_epoch: i64,
    pub run_status: String,
    pub control_epoch: i64,
}

pub(super) async fn load_count_attempt<C: ConnectionTrait>(
    connection: &C,
    effect_attempt_id: &str,
    fence: &EffectAttemptFence,
) -> StorageResult<FencedCountCall> {
    let row = connection
        .query_one(sql(
            "SELECT ea.status AS attempt_status, ea.provider_request_id AS attempt_provider_request_id, attempt_result.content_hash AS attempt_result_digest, attempt_error.content_hash AS attempt_error_digest, e.id AS effect_id, e.status AS effect_status, e.count_call_id, e.node_instance_id, cc.count_ordinal, cc.status AS count_status, cc.count_execution_pin_digest, cc.trim_candidate_object_id, cc.trim_candidate_digest, cc.request_digest, cc.operation_key_json, cc.result_source, cc.result_object_id, result.content_hash AS result_digest, cp.checkpoint_digest, a.status AS node_attempt_status, a.worker_id, a.lease_fence, a.run_control_epoch, r.status AS run_status, r.control_epoch FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id JOIN count_calls cc ON cc.id = e.count_call_id JOIN node_attempts a ON a.id = ea.invoking_node_attempt_id JOIN node_instances ni ON ni.id = e.node_instance_id JOIN graph_runs r ON r.id = ni.run_id LEFT JOIN content_objects attempt_result ON attempt_result.id = ea.result_object_id LEFT JOIN content_objects attempt_error ON attempt_error.id = ea.error_object_id LEFT JOIN content_objects result ON result.id = cc.result_object_id LEFT JOIN llm_loop_checkpoints cp ON cp.node_instance_id = e.node_instance_id WHERE ea.id = ? AND ea.invoking_node_attempt_id = ?",
            vec![
                effect_attempt_id.into(),
                fence.invoking_node_attempt_id.clone().into(),
            ],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "count_effect_attempt",
            id: effect_attempt_id.into(),
        })?;
    let operation: zhuangsheng_core::llm::OperationKey =
        serde_json::from_str(&row.try_get::<String>("", "operation_key_json")?)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
    Ok(FencedCountCall {
        effect_id: row.try_get("", "effect_id")?,
        count_call_id: row.try_get("", "count_call_id")?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        count_ordinal: u64::try_from(row.try_get::<i64>("", "count_ordinal")?)
            .map_err(|_| StorageError::Integrity("invalid count ordinal".into()))?,
        pin_digest: row.try_get("", "count_execution_pin_digest")?,
        candidate_ref: row.try_get("", "trim_candidate_object_id")?,
        candidate_digest: row.try_get("", "trim_candidate_digest")?,
        request_digest: row.try_get("", "request_digest")?,
        attempt_status: row.try_get("", "attempt_status")?,
        effect_status: row.try_get("", "effect_status")?,
        count_status: row.try_get("", "count_status")?,
        provider_count_available: operation.operation == Operation::CountTokens,
        attempt_provider_request_id: row.try_get("", "attempt_provider_request_id")?,
        result_object_id: row.try_get("", "result_object_id")?,
        result_digest: row.try_get("", "result_digest")?,
        error_digest: row.try_get("", "attempt_error_digest")?,
        result_source: row.try_get("", "result_source")?,
        checkpoint_digest: row.try_get("", "checkpoint_digest")?,
        node_attempt_status: row.try_get("", "node_attempt_status")?,
        worker_id: row.try_get("", "worker_id")?,
        lease_fence: row.try_get("", "lease_fence")?,
        attempt_epoch: row.try_get("", "run_control_epoch")?,
        run_status: row.try_get("", "run_status")?,
        control_epoch: row.try_get("", "control_epoch")?,
    })
}

pub(super) fn validate_count_fence(
    call: &FencedCountCall,
    fence: &EffectAttemptFence,
) -> StorageResult<()> {
    if call.worker_id.as_deref() != Some(&fence.worker_id)
        || u64::try_from(call.lease_fence).ok() != Some(fence.lease_fence)
        || u64::try_from(call.attempt_epoch).ok() != Some(fence.run_control_epoch)
        || call.attempt_epoch != call.control_epoch
        || call.node_attempt_status != "running"
        || call.run_status != "running"
    {
        return Err(StorageError::Conflict("effect_attempt_fence"));
    }
    Ok(())
}

pub(super) fn validate_count_replay_fence(
    call: &FencedCountCall,
    fence: &EffectAttemptFence,
) -> StorageResult<()> {
    if call
        .worker_id
        .as_deref()
        .is_some_and(|worker| worker != fence.worker_id)
        || u64::try_from(call.lease_fence).ok() != Some(fence.lease_fence)
        || u64::try_from(call.attempt_epoch).ok() != Some(fence.run_control_epoch)
    {
        return Err(StorageError::Conflict("effect_attempt_fence"));
    }
    Ok(())
}
