use thiserror::Error;

use crate::graph::MemoryToolCapability;

use super::{
    ExecuteMemorySearchToolBatchCommand, LlmLoopCheckpoint, MemoryProposalToolCallCommand,
    MemoryProposalToolCallDigestMaterial, MemoryProposalToolInput, MemorySearchToolCallCommand,
    MemorySearchToolCallDigestMaterial, PrepareMemoryProposalToolBatchCommand, ResolvedMemoryTool,
    TOOL_CALL_POLICY_VERSION, ToolCallCheckpoint, ToolCallCheckpointStatus,
    ir::LlmTurnItemIr,
    memory_tool_batch_validation::{normalize_proposal, parse_search_arguments},
};

pub enum MemoryToolBatchPlan {
    Search(ExecuteMemorySearchToolBatchCommand),
    Proposal(PrepareMemoryProposalToolBatchCommand),
}

pub struct MemoryToolBatchInput<'a> {
    pub tools: &'a [ResolvedMemoryTool],
    pub response_items: &'a [LlmTurnItemIr],
    pub model_call_id: &'a str,
    pub node_instance_id: &'a str,
    pub originating_attempt_id: &'a str,
    pub checkpoint: LlmLoopCheckpoint,
    pub max_tool_calls: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct MemoryToolBatchError {
    pub code: &'static str,
    pub message: String,
}

impl MemoryToolBatchError {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub fn plan_memory_tool_batch(
    input: MemoryToolBatchInput<'_>,
) -> Result<Option<MemoryToolBatchPlan>, MemoryToolBatchError> {
    let calls: Vec<_> = input
        .response_items
        .iter()
        .filter_map(|item| match item {
            LlmTurnItemIr::AssistantToolCall { call, .. } => Some(call),
            _ => None,
        })
        .collect();
    let resolved: Vec<_> = calls
        .iter()
        .map(|call| {
            input
                .tools
                .iter()
                .find(|tool| tool.exposed_name == call.name)
        })
        .collect();
    if resolved.iter().all(Option::is_none) {
        return Ok(None);
    }
    if resolved.iter().any(Option::is_none) {
        return Err(error(
            "mixed_memory_tool_batch",
            "memory capability calls cannot be mixed with other tools in one model batch",
        ));
    }
    let resolved: Vec<_> = resolved.into_iter().map(Option::unwrap).collect();
    let capability = resolved[0].grant.capability;
    if resolved
        .iter()
        .any(|tool| tool.grant.capability != capability)
    {
        return Err(error(
            "mixed_memory_capability_batch",
            "search and proposal calls must use separate model batches",
        ));
    }
    let used = input
        .checkpoint
        .tool_calls_used
        .checked_add(calls.len() as u64)
        .filter(|used| *used <= input.max_tool_calls)
        .ok_or_else(|| error("tool_call_limit_exceeded", "tool-call limit exceeded"))?;
    match capability {
        MemoryToolCapability::SearchMemory => plan_search(input, calls, resolved, used).map(Some),
        MemoryToolCapability::ProposeMemoryChange => {
            plan_proposals(input, calls, resolved, used).map(Some)
        }
    }
}

fn plan_search(
    input: MemoryToolBatchInput<'_>,
    calls: Vec<&super::ir::ToolCallIr>,
    tools: Vec<&ResolvedMemoryTool>,
    used: u64,
) -> Result<MemoryToolBatchPlan, MemoryToolBatchError> {
    let node_instance_id = input.node_instance_id.to_owned();
    let originating_attempt_id = input.originating_attempt_id.to_owned();
    let model_call_id = input.model_call_id.to_owned();
    let mut planned = Vec::with_capacity(calls.len());
    for (index, (call, tool)) in calls.iter().zip(tools).enumerate() {
        let query = parse_search_arguments(call.arguments.clone(), &tool.grant)?;
        planned.push(MemorySearchToolCallCommand {
            tool_call_id: format!("memorysearch_{}_{}", input.model_call_id, index),
            provider_call_id: call.provider_call_id.clone(),
            call_index: index as u64,
            call_digest: MemorySearchToolCallDigestMaterial {
                query: query.clone(),
                grant: tool.grant.clone(),
                policy_version: TOOL_CALL_POLICY_VERSION,
            }
            .digest()
            .map_err(|e| error("memory_tool_digest_failed", e.to_string()))?,
            query,
        });
    }
    let checkpoint = terminal_checkpoint(
        input,
        &planned
            .iter()
            .map(|call| (&call.tool_call_id, call.call_index, &call.call_digest))
            .collect::<Vec<_>>(),
        used,
    )?;
    Ok(MemoryToolBatchPlan::Search(
        ExecuteMemorySearchToolBatchCommand {
            node_instance_id,
            originating_attempt_id,
            model_call_id,
            calls: planned,
            checkpoint,
        },
    ))
}

fn plan_proposals(
    input: MemoryToolBatchInput<'_>,
    calls: Vec<&super::ir::ToolCallIr>,
    tools: Vec<&ResolvedMemoryTool>,
    used: u64,
) -> Result<MemoryToolBatchPlan, MemoryToolBatchError> {
    let wait_id = format!("wait_memory_review_{}", input.model_call_id);
    let mut planned = Vec::with_capacity(calls.len());
    for (index, (call, tool)) in calls.iter().zip(tools).enumerate() {
        let mut proposal: MemoryProposalToolInput = serde_json::from_value(call.arguments.clone())
            .map_err(|_| {
                error(
                    "memory_proposal_arguments_invalid",
                    "propose_memory_change arguments are invalid",
                )
            })?;
        normalize_proposal(&mut proposal, &tool.grant)?;
        planned.push(MemoryProposalToolCallCommand {
            tool_call_id: format!("memoryproposal_{}_{}", input.model_call_id, index),
            provider_call_id: call.provider_call_id.clone(),
            call_index: index as u64,
            call_digest: MemoryProposalToolCallDigestMaterial {
                input: proposal.clone(),
                grant: tool.grant.clone(),
                policy_version: TOOL_CALL_POLICY_VERSION,
            }
            .digest()
            .map_err(|e| error("memory_tool_digest_failed", e.to_string()))?,
            input: proposal,
        });
    }
    let mut checkpoint = input.checkpoint;
    checkpoint.current_batch = planned
        .iter()
        .map(|call| ToolCallCheckpoint {
            tool_call_id: call.tool_call_id.clone(),
            call_index: call.call_index,
            call_digest: call.call_digest.clone(),
            status: ToolCallCheckpointStatus::AwaitingApproval,
            effect_id: None,
            output_ref: None,
            wait_id: Some(wait_id.clone()),
        })
        .collect();
    checkpoint.tool_calls_used = used;
    checkpoint.effect_watermark = wait_id.clone();
    checkpoint.wait_ids.push(wait_id.clone());
    checkpoint = checkpoint
        .seal()
        .map_err(|e| error("memory_tool_checkpoint_invalid", e.to_string()))?;
    Ok(MemoryToolBatchPlan::Proposal(
        PrepareMemoryProposalToolBatchCommand {
            wait_id,
            node_instance_id: input.node_instance_id.into(),
            originating_attempt_id: input.originating_attempt_id.into(),
            model_call_id: input.model_call_id.into(),
            calls: planned,
            checkpoint,
        },
    ))
}

fn terminal_checkpoint(
    input: MemoryToolBatchInput<'_>,
    calls: &[(&String, u64, &String)],
    used: u64,
) -> Result<LlmLoopCheckpoint, MemoryToolBatchError> {
    let mut checkpoint = input.checkpoint;
    checkpoint.current_batch = calls
        .iter()
        .map(|(id, index, digest)| ToolCallCheckpoint {
            tool_call_id: (*id).clone(),
            call_index: *index,
            call_digest: (*digest).clone(),
            status: ToolCallCheckpointStatus::Completed,
            effect_id: None,
            output_ref: None,
            wait_id: None,
        })
        .collect();
    checkpoint.tool_calls_used = used;
    checkpoint.effect_watermark = calls
        .last()
        .map(|call| (*call.0).clone())
        .unwrap_or_default();
    checkpoint
        .seal()
        .map_err(|e| error("memory_tool_checkpoint_invalid", e.to_string()))
}

fn error(code: &'static str, message: impl Into<String>) -> MemoryToolBatchError {
    MemoryToolBatchError::new(code, message)
}
