use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    canonical,
    graph::{ToolApprovalPolicy, ToolGrant},
    llm::{
        EffectAttemptFence, LlmLogicalCallStatus, LlmLoopCheckpoint, PrepareToolApprovalCall,
        PrepareToolCallCommand, TOOL_CALL_POLICY_VERSION, ToolCallCheckpointStatus,
        ToolCallDigestMaterial,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::validation::LedgerContext;

pub(super) struct ValidatedToolCall {
    pub arguments: serde_json::Value,
    pub grant: ToolGrant,
    pub requires_approval: bool,
}

pub(super) fn validate_tool_material(
    context: &LedgerContext,
    command: &PrepareToolCallCommand,
) -> StorageResult<ValidatedToolCall> {
    validate_material(
        context,
        &command.checkpoint,
        ToolMaterialInput {
            binding_id: &command.binding_id,
            tool_id: &command.tool_id,
            tool_version: &command.tool_version,
            call_digest: &command.call_digest,
            arguments_bytes: &command.arguments_bytes,
            descriptor_digest: &command.descriptor_digest,
            schema_compilation_digests: &command.schema_compilation_digests,
            implementation_digest: &command.implementation_digest,
            descriptor_requires_approval: command.descriptor_requires_approval,
        },
    )
}

pub(super) fn validate_approval_tool_material(
    context: &LedgerContext,
    checkpoint: &LlmLoopCheckpoint,
    command: &PrepareToolApprovalCall,
) -> StorageResult<ValidatedToolCall> {
    validate_material(
        context,
        checkpoint,
        ToolMaterialInput {
            binding_id: &command.binding_id,
            tool_id: &command.tool_id,
            tool_version: &command.tool_version,
            call_digest: &command.call_digest,
            arguments_bytes: &command.arguments_bytes,
            descriptor_digest: &command.descriptor_digest,
            schema_compilation_digests: &command.schema_compilation_digests,
            implementation_digest: &command.implementation_digest,
            descriptor_requires_approval: command.descriptor_requires_approval,
        },
    )
}

struct ToolMaterialInput<'a> {
    binding_id: &'a str,
    tool_id: &'a str,
    tool_version: &'a str,
    call_digest: &'a str,
    arguments_bytes: &'a [u8],
    descriptor_digest: &'a str,
    schema_compilation_digests: &'a [String],
    implementation_digest: &'a str,
    descriptor_requires_approval: bool,
}

fn validate_material(
    context: &LedgerContext,
    checkpoint: &LlmLoopCheckpoint,
    command: ToolMaterialInput<'_>,
) -> StorageResult<ValidatedToolCall> {
    let grant = context
        .snapshot
        .tools
        .iter()
        .find(|grant| grant.binding_id == command.binding_id)
        .cloned()
        .ok_or_else(|| StorageError::InvalidArgument("tool binding is not pinned".into()))?;
    let registry = checkpoint
        .registry_snapshot
        .entries
        .iter()
        .find(|entry| entry.tool_id == command.tool_id && entry.version == command.tool_version)
        .ok_or_else(|| StorageError::InvalidArgument("tool registry entry is not pinned".into()))?;
    let arguments: serde_json::Value = serde_json::from_slice(command.arguments_bytes)
        .map_err(|_| StorageError::InvalidArgument("tool arguments are not valid JSON".into()))?;
    let canonical_arguments = canonical::to_vec(&arguments)?;
    let effective_approval =
        command.descriptor_requires_approval || grant.approval == Some(ToolApprovalPolicy::Always);
    let material = ToolCallDigestMaterial {
        binding_id: command.binding_id.to_owned(),
        tool_id: command.tool_id.to_owned(),
        tool_version: command.tool_version.to_owned(),
        arguments: arguments.clone(),
        grant: grant.clone(),
        descriptor_digest: command.descriptor_digest.to_owned(),
        schema_compilation_digests: command.schema_compilation_digests.to_vec(),
        implementation_digest: command.implementation_digest.to_owned(),
        policy_version: TOOL_CALL_POLICY_VERSION,
    };
    if grant.tool_id != command.tool_id
        || grant.version != command.tool_version
        || registry.descriptor_digest != command.descriptor_digest
        || registry.schema_compilation_digests != command.schema_compilation_digests
        || registry.implementation_digest != command.implementation_digest
        || canonical_arguments != command.arguments_bytes
        || material.digest()? != command.call_digest
    {
        return Err(StorageError::InvalidArgument(
            "tool call material does not match its pinned grant and registry entry".into(),
        ));
    }
    Ok(ValidatedToolCall {
        arguments,
        grant,
        requires_approval: effective_approval,
    })
}

pub(super) struct ToolCheckpointExpectation<'a> {
    pub context: &'a LedgerContext,
    pub node_instance_id: &'a str,
    pub updater_attempt_id: &'a str,
    pub model_call_id: &'a str,
    pub tool_call_id: &'a str,
    pub effect_id: &'a str,
    pub effect_attempt_id: &'a str,
    pub call_index: u64,
    pub call_digest: &'a str,
    pub expected_tool_calls_used: u64,
    pub status: ToolCallCheckpointStatus,
    pub output_ref: Option<&'a str>,
}

pub(super) fn validate_tool_checkpoint(
    checkpoint: &LlmLoopCheckpoint,
    expected: ToolCheckpointExpectation<'_>,
) -> StorageResult<()> {
    let call = checkpoint
        .current_batch
        .iter()
        .find(|call| call.tool_call_id == expected.tool_call_id)
        .ok_or_else(|| StorageError::InvalidArgument("tool checkpoint call is missing".into()))?;
    let max_tool_calls = expected
        .context
        .snapshot
        .limits
        .max_tool_calls
        .ok_or_else(|| StorageError::Integrity("tool-call limit is not pinned".into()))?;
    let model_matches = checkpoint
        .active_model_effect
        .as_ref()
        .is_some_and(|active| {
            active.model_call_id == expected.model_call_id
                && active.status == LlmLogicalCallStatus::Completed
        });
    let ordered_unique = checkpoint
        .current_batch
        .windows(2)
        .all(|pair| pair[0].call_index < pair[1].call_index);
    if checkpoint.schema_version != 1
        || !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != expected.node_instance_id
        || checkpoint.last_updated_by_attempt_id != expected.updater_attempt_id
        || checkpoint.graph_revision_id != expected.context.graph_revision_id
        || checkpoint.context_snapshot_ref != expected.context.execution_snapshot_object_id
        || checkpoint.tool_calls_used != expected.expected_tool_calls_used
        || checkpoint.tool_calls_used > max_tool_calls
        || checkpoint.effect_watermark != expected.effect_attempt_id
        || call.call_index != expected.call_index
        || call.call_digest != expected.call_digest
        || call.status != expected.status
        || call.effect_id.as_deref() != Some(expected.effect_id)
        || call.output_ref.as_deref() != expected.output_ref
        || call.wait_id.is_some()
        || !model_matches
        || !ordered_unique
    {
        return Err(StorageError::InvalidArgument(
            "LLM checkpoint is incompatible with tool transition".into(),
        ));
    }
    Ok(())
}

pub(super) struct FencedToolCall {
    pub effect_id: String,
    pub tool_call_id: String,
    pub model_call_id: String,
    pub node_instance_id: String,
    pub call_index: u64,
    pub call_digest: String,
    pub attempt_status: String,
    pub effect_status: String,
    pub tool_status: String,
    pub classification: String,
    pub attempt_provider_request_id: Option<String>,
    pub output_object_id: Option<String>,
    pub output_digest: Option<String>,
    pub error_digest: Option<String>,
    pub checkpoint_digest: Option<String>,
    pub tool_calls_used: u64,
    pub node_attempt_status: String,
    pub worker_id: Option<String>,
    pub lease_fence: i64,
    pub attempt_epoch: i64,
    pub run_status: String,
    pub control_epoch: i64,
}

pub(super) async fn load_tool_attempt<C: ConnectionTrait>(
    connection: &C,
    effect_attempt_id: &str,
    fence: &EffectAttemptFence,
) -> StorageResult<FencedToolCall> {
    let row = connection
        .query_one(sql(
            "SELECT ea.status AS attempt_status, ea.provider_request_id AS attempt_provider_request_id, attempt_result.content_hash AS attempt_result_digest, attempt_error.content_hash AS attempt_error_digest, e.id AS effect_id, e.status AS effect_status, e.classification, e.tool_call_id, e.node_instance_id, tc.model_call_id, tc.call_index, tc.call_digest, tc.arguments_object_id, tc.status AS tool_status, tc.output_object_id, output.content_hash AS output_digest, cp.checkpoint_digest, a.status AS node_attempt_status, a.worker_id, a.lease_fence, a.run_control_epoch, r.status AS run_status, r.control_epoch, (SELECT COUNT(*) FROM tool_calls all_calls WHERE all_calls.node_instance_id = e.node_instance_id) AS tool_calls_used FROM effect_attempts ea JOIN effects e ON e.id = ea.effect_id JOIN tool_calls tc ON tc.id = e.tool_call_id JOIN node_attempts a ON a.id = ea.invoking_node_attempt_id JOIN node_instances ni ON ni.id = e.node_instance_id JOIN graph_runs r ON r.id = ni.run_id LEFT JOIN content_objects attempt_result ON attempt_result.id = ea.result_object_id LEFT JOIN content_objects attempt_error ON attempt_error.id = ea.error_object_id LEFT JOIN content_objects output ON output.id = tc.output_object_id LEFT JOIN llm_loop_checkpoints cp ON cp.node_instance_id = e.node_instance_id WHERE ea.id = ? AND ea.invoking_node_attempt_id = ?",
            vec![
                effect_attempt_id.into(),
                fence.invoking_node_attempt_id.clone().into(),
            ],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "tool_effect_attempt",
            id: effect_attempt_id.into(),
        })?;
    Ok(FencedToolCall {
        effect_id: row.try_get("", "effect_id")?,
        tool_call_id: row.try_get("", "tool_call_id")?,
        model_call_id: row.try_get("", "model_call_id")?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        call_index: u64::try_from(row.try_get::<i64>("", "call_index")?)
            .map_err(|_| StorageError::Integrity("invalid tool call index".into()))?,
        call_digest: row.try_get("", "call_digest")?,
        attempt_status: row.try_get("", "attempt_status")?,
        effect_status: row.try_get("", "effect_status")?,
        tool_status: row.try_get("", "tool_status")?,
        classification: row.try_get("", "classification")?,
        attempt_provider_request_id: row.try_get("", "attempt_provider_request_id")?,
        output_object_id: row.try_get("", "output_object_id")?,
        output_digest: row.try_get("", "output_digest")?,
        error_digest: row.try_get("", "attempt_error_digest")?,
        checkpoint_digest: row.try_get("", "checkpoint_digest")?,
        tool_calls_used: u64::try_from(row.try_get::<i64>("", "tool_calls_used")?)
            .map_err(|_| StorageError::Integrity("invalid tool-call count".into()))?,
        node_attempt_status: row.try_get("", "node_attempt_status")?,
        worker_id: row.try_get("", "worker_id")?,
        lease_fence: row.try_get("", "lease_fence")?,
        attempt_epoch: row.try_get("", "run_control_epoch")?,
        run_status: row.try_get("", "run_status")?,
        control_epoch: row.try_get("", "control_epoch")?,
    })
}

pub(super) fn validate_tool_fence(
    call: &FencedToolCall,
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

pub(super) fn validate_tool_replay_fence(
    call: &FencedToolCall,
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

pub(super) async fn validate_tool_start_policy<C: ConnectionTrait>(
    connection: &C,
    context: &LedgerContext,
    call: &FencedToolCall,
) -> StorageResult<()> {
    let max_concurrent = context
        .snapshot
        .limits
        .max_concurrent_tools
        .ok_or_else(|| StorageError::Integrity("tool concurrency limit is not pinned".into()))?;
    let rows = connection.query_all(sql(
        "SELECT tc.id, tc.call_index, tc.status, e.classification FROM tool_calls tc LEFT JOIN effects e ON e.tool_call_id = tc.id WHERE tc.model_call_id = ? ORDER BY tc.call_index",
        vec![call.model_call_id.clone().into()],
    )).await?;
    let running = rows
        .iter()
        .filter(|row| row.try_get::<String>("", "status").as_deref() == Ok("running"))
        .count();
    if u64::try_from(running)
        .ok()
        .is_none_or(|count| count >= max_concurrent)
    {
        return Err(StorageError::Conflict("tool_concurrency_limit"));
    }
    let current_non_idempotent = call.classification == "non_idempotent";
    for row in rows {
        let id: String = row.try_get("", "id")?;
        if id == call.tool_call_id {
            continue;
        }
        let status: String = row.try_get("", "status")?;
        let classification: Option<String> = row.try_get("", "classification")?;
        let call_index = u64::try_from(row.try_get::<i64>("", "call_index")?)
            .map_err(|_| StorageError::Integrity("invalid sibling tool call index".into()))?;
        if status == "outcome_unknown" {
            return Err(StorageError::Conflict("tool_batch_outcome_unknown"));
        }
        if status == "running" && classification.as_deref() == Some("non_idempotent") {
            return Err(StorageError::Conflict("tool_non_idempotent_serial"));
        }
        if current_non_idempotent && status == "running" {
            return Err(StorageError::Conflict("tool_non_idempotent_serial"));
        }
        let terminal = matches!(
            status.as_str(),
            "completed" | "failed" | "denied" | "cancelled_before_start" | "abandoned_unknown"
        );
        if call_index < call.call_index
            && !terminal
            && (current_non_idempotent || classification.as_deref() == Some("non_idempotent"))
        {
            return Err(StorageError::Conflict("tool_non_idempotent_serial"));
        }
    }
    Ok(())
}
