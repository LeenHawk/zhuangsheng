use std::collections::BTreeSet;

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    canonical,
    graph::MemoryToolCapability,
    llm::{
        LlmLogicalCallStatus, MemoryProposalToolCallDigestMaterial,
        PrepareMemoryProposalToolBatchCommand, TOOL_CALL_POLICY_VERSION, ToolCallCheckpointStatus,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::validation::LedgerContext;

pub(super) async fn validate_memory_proposal_batch<C: ConnectionTrait>(
    connection: &C,
    context: &LedgerContext,
    command: &PrepareMemoryProposalToolBatchCommand,
) -> StorageResult<(String, String)> {
    validate_fields(command)?;
    let owner = connection.query_one_raw(sql(
        "SELECT ni.run_id,ni.node_id,ni.status AS instance_status,a.status AS attempt_status,r.status AS run_status,mc.status AS model_status,mc.node_instance_id AS model_instance FROM node_instances ni JOIN node_attempts a ON a.id=? JOIN graph_runs r ON r.id=ni.run_id JOIN model_calls mc ON mc.id=? WHERE ni.id=? AND a.node_instance_id=ni.id",
        vec![command.originating_attempt_id.clone().into(),command.model_call_id.clone().into(),command.node_instance_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "memory_proposal_tool_owner", id: command.node_instance_id.clone() })?;
    if owner.try_get::<String>("", "instance_status")? != "running"
        || owner.try_get::<String>("", "attempt_status")? != "running"
        || owner.try_get::<String>("", "run_status")? != "running"
        || owner.try_get::<String>("", "model_status")? != "completed"
        || owner.try_get::<String>("", "model_instance")? != command.node_instance_id
    {
        return Err(StorageError::Conflict("memory_proposal_tool_owner"));
    }
    for call in &command.calls {
        let memory =
            context.snapshot.memory.as_ref().ok_or_else(|| {
                StorageError::InvalidArgument("memory tools are not pinned".into())
            })?;
        let grants: Vec<_> = memory
            .tools
            .iter()
            .filter(|grant| {
                grant.capability == MemoryToolCapability::ProposeMemoryChange
                    && grant.scopes.contains(&call.input.scope_id)
            })
            .collect();
        if grants.len() != 1 {
            return Err(StorageError::InvalidArgument(
                "propose_memory_change scope grant is missing or ambiguous".into(),
            ));
        }
        let grant = grants[0];
        if canonical::to_vec(&call.input)?.len() as u64 > grant.max_proposal_bytes.unwrap_or(0)
            || (MemoryProposalToolCallDigestMaterial {
                input: call.input.clone(),
                grant: grant.clone(),
                policy_version: TOOL_CALL_POLICY_VERSION,
            })
            .digest()?
                != call.call_digest
        {
            return Err(StorageError::InvalidArgument(
                "propose_memory_change call does not match its pinned grant".into(),
            ));
        }
    }
    let existing: i64 = connection
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM tool_calls WHERE model_call_id=?",
            vec![command.model_call_id.clone().into()],
        ))
        .await?
        .expect("count row")
        .try_get("", "count")?;
    if existing != 0 {
        return Err(StorageError::Conflict("memory_proposal_batch_exists"));
    }
    if connection
        .query_one_raw(sql(
            "SELECT 1 AS present FROM node_waits WHERE node_instance_id=? AND status='open'",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .is_some()
    {
        return Err(StorageError::Conflict("node_instance_open_wait"));
    }
    let existing_total: i64 = connection
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM tool_calls WHERE node_instance_id=?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .expect("count row")
        .try_get("", "count")?;
    let expected_used = u64::try_from(existing_total)
        .ok()
        .and_then(|value| value.checked_add(command.calls.len() as u64))
        .ok_or_else(|| StorageError::Integrity("tool-call count overflow".into()))?;
    if expected_used
        > context
            .snapshot
            .limits
            .max_tool_calls
            .ok_or_else(|| StorageError::Integrity("tool-call limit is not pinned".into()))?
    {
        return Err(StorageError::InvalidArgument(
            "tool-call limit exceeded".into(),
        ));
    }
    let mut expected_waits: Vec<String> = connection
        .query_all_raw(sql(
            "SELECT id FROM node_waits WHERE node_instance_id=? ORDER BY created_at,id",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get("", "id"))
        .collect::<Result<_, _>>()?;
    expected_waits.push(command.wait_id.clone());
    validate_checkpoint(context, command, expected_used, &expected_waits)?;
    Ok((owner.try_get("", "run_id")?, owner.try_get("", "node_id")?))
}

fn validate_fields(command: &PrepareMemoryProposalToolBatchCommand) -> StorageResult<()> {
    if command.calls.is_empty()
        || command.calls.len() > 32
        || [
            &command.wait_id,
            &command.node_instance_id,
            &command.originating_attempt_id,
            &command.model_call_id,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256)
    {
        return Err(StorageError::InvalidArgument(
            "memory proposal batch is outside supported bounds".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    for (index, call) in command.calls.iter().enumerate() {
        if call.tool_call_id.is_empty()
            || call.tool_call_id.len() > 256
            || call.call_index != index as u64
            || call.call_digest.is_empty()
            || !ids.insert(&call.tool_call_id)
        {
            return Err(StorageError::InvalidArgument(
                "memory proposal calls must be a complete ordered batch".into(),
            ));
        }
    }
    Ok(())
}

fn validate_checkpoint(
    context: &LedgerContext,
    command: &PrepareMemoryProposalToolBatchCommand,
    expected_used: u64,
    expected_waits: &[String],
) -> StorageResult<()> {
    let checkpoint = &command.checkpoint;
    let active = checkpoint
        .active_model_effect
        .as_ref()
        .is_some_and(|active| {
            active.model_call_id == command.model_call_id
                && active.status == LlmLogicalCallStatus::Completed
        });
    let calls = checkpoint.current_batch.len() == command.calls.len()
        && checkpoint
            .current_batch
            .iter()
            .zip(&command.calls)
            .all(|(stored, call)| {
                stored.tool_call_id == call.tool_call_id
                    && stored.call_index == call.call_index
                    && stored.call_digest == call.call_digest
                    && stored.status == ToolCallCheckpointStatus::AwaitingApproval
                    && stored.effect_id.is_none()
                    && stored.output_ref.is_none()
                    && stored.wait_id.as_deref() == Some(&command.wait_id)
            });
    if !checkpoint.checksum_is_valid()
        || checkpoint.node_instance_id != command.node_instance_id
        || checkpoint.last_updated_by_attempt_id != command.originating_attempt_id
        || checkpoint.graph_revision_id != context.graph_revision_id
        || checkpoint.context_snapshot_ref != context.execution_snapshot_object_id
        || checkpoint.tool_calls_used != expected_used
        || !active
        || !calls
        || checkpoint.wait_ids != expected_waits
        || checkpoint.effect_watermark != command.wait_id
    {
        return Err(StorageError::InvalidArgument(
            "memory proposal checkpoint is incompatible".into(),
        ));
    }
    Ok(())
}
