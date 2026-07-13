use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::memory::{DecideMemoryProposalCommand, MemoryProposalDecision as DomainDecision},
    canonical,
    llm::{LlmLoopCheckpoint, ToolCallCheckpointStatus},
    memory::MemoryProposalStatus,
    runtime::{
        MemoryProposalDecision, SubmitWaitResponseCommand, ToolApprovalDecisionKind,
        WaitDeliveryStatus, WaitDeliveryView,
    },
    state::{ActorKind, ActorRef},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    memory::{decide_in, load_proposal},
    runtime::{Event, ResumeAttempt, add_object_ref, append_event, create_resume_attempt},
};

use super::{
    memory_proposal_tool::MemoryProposalContinuation,
    model_ledger_helpers::{add_ref, persist_checkpoint},
    tool_ledger_finish::validate_tool_output,
    wait_delivery::WaitContext,
};

pub(super) struct SettledMemoryProposalResponse {
    pub response_ref: String,
    pub view: WaitDeliveryView,
    pub resume_attempt_id: Option<String>,
}

pub(super) async fn settle_memory_proposal_response<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    context: &WaitContext,
    decisions: &[MemoryProposalDecision],
    now: i64,
) -> StorageResult<SettledMemoryProposalResponse> {
    let continuation: MemoryProposalContinuation =
        load_object_json(connection, &context.continuation_ref).await?;
    validate_continuation(connection, command, context, &continuation, decisions).await?;
    let resume_attempt_id = create_resume_attempt(
        connection,
        ResumeAttempt {
            node_instance_id: &context.node_instance_id,
            source_attempt_id: &context.node_attempt_id,
            run_id: &context.run_id,
            control_epoch: context.control_epoch,
            idempotency_key: &format!("wait:{}:resume", command.delivery_id),
        },
        now,
    )
    .await?;
    let actor = ActorRef {
        kind: if command.actor_kind == "human" {
            ActorKind::User
        } else {
            ActorKind::Application
        },
        id: command.actor_id.clone(),
    };
    let supplied: BTreeMap<_, _> = decisions
        .iter()
        .map(|decision| (decision.proposal_id.as_str(), decision))
        .collect();
    let mut checkpoint =
        load_checkpoint(connection, context, &command.wait_id, &continuation).await?;
    let mut decision_refs = Vec::with_capacity(continuation.calls.len());
    let mut proposal_ids = Vec::with_capacity(continuation.calls.len());
    for plan in &continuation.calls {
        let decision = supplied[plan.proposal_id.as_str()];
        let proposal = load_proposal(connection, &plan.proposal_id).await?;
        let expected_status = proposal.status;
        let domain_decision = match decision.decision {
            ToolApprovalDecisionKind::Approve => DomainDecision::Approve,
            ToolApprovalDecisionKind::Reject => DomainDecision::Reject,
        };
        let proposal = decide_in(
            connection,
            &DecideMemoryProposalCommand {
                proposal_id: plan.proposal_id.clone(),
                expected_status,
                decision: domain_decision,
                actor: actor.clone(),
                idempotency_key: format!(
                    "wait:{}:{}:{}",
                    command.wait_id, command.delivery_id, plan.proposal_id
                ),
            },
            now,
        )
        .await?;
        let decision_ref = persist_decision(connection, command, decision, &proposal, now).await?;
        settle_blocker(connection, command, decision, &decision_ref, now).await?;
        let output_ref = persist_tool_output(connection, plan, &proposal, now).await?;
        append_event(
            connection,
            Event {
                run_id: &context.run_id,
                event_type: "tool.call.completed",
                importance: "critical",
                node_instance_id: Some(&context.node_instance_id),
                attempt_id: Some(&context.node_attempt_id),
                payload: json!({
                    "schemaVersion":1,
                    "waitId":command.wait_id,
                    "toolCallId":plan.tool_call_id,
                    "callIndex":plan.call_index,
                    "proposalId":plan.proposal_id,
                    "outputRef":output_ref,
                }),
                now,
            },
        )
        .await?;
        let call = checkpoint
            .current_batch
            .iter_mut()
            .find(|call| call.tool_call_id == plan.tool_call_id)
            .ok_or_else(|| {
                StorageError::Integrity("memory proposal checkpoint call is missing".into())
            })?;
        call.status = ToolCallCheckpointStatus::Completed;
        call.output_ref = Some(output_ref);
        call.wait_id = None;
        decision_refs.push(decision_ref);
        proposal_ids.push(proposal.id);
    }
    checkpoint.wait_ids.retain(|id| id != &command.wait_id);
    checkpoint.last_updated_by_attempt_id = resume_attempt_id.clone();
    checkpoint.effect_watermark = format!("memory-review:{}", command.delivery_id);
    checkpoint = checkpoint.seal()?;
    persist_checkpoint(connection, &checkpoint, now).await?;
    if connection
        .execute_raw(sql(
            "UPDATE node_instances SET status='ready',updated_at=? WHERE id=? AND status='waiting'",
            vec![now.into(), context.node_instance_id.clone().into()],
        ))
        .await?
        .rows_affected()
        != 1
    {
        return Err(StorageError::Conflict("memory_proposal_wait_owner"));
    }
    let response_ref =
        persist_response(connection, command, &decision_refs, &proposal_ids, now).await?;
    Ok(SettledMemoryProposalResponse {
        response_ref,
        view: WaitDeliveryView {
            wait_id: command.wait_id.clone(),
            delivery_id: command.delivery_id.clone(),
            status: WaitDeliveryStatus::Resolved,
            prepared_tool_call_ids: Vec::new(),
            denied_tool_call_ids: Vec::new(),
            decided_memory_proposal_ids: proposal_ids,
            replayed: false,
        },
        resume_attempt_id: Some(resume_attempt_id),
    })
}

async fn validate_continuation<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    context: &WaitContext,
    continuation: &MemoryProposalContinuation,
    decisions: &[MemoryProposalDecision],
) -> StorageResult<()> {
    if continuation.schema_version != 1
        || continuation.node_instance_id != context.node_instance_id
        || continuation.originating_attempt_id != context.node_attempt_id
        || continuation.calls.is_empty()
    {
        return Err(StorageError::Integrity(
            "memory proposal continuation is incompatible".into(),
        ));
    }
    let blockers=connection.query_all_raw(sql("SELECT blocker_id FROM wait_blockers WHERE wait_id=? AND blocker_kind='memory_proposal' AND status='open' ORDER BY blocker_order",vec![command.wait_id.clone().into()])).await?;
    if blockers.len() != continuation.calls.len() || decisions.len() != blockers.len() {
        return Err(StorageError::InvalidArgument(
            "memory proposal response must cover every open blocker".into(),
        ));
    }
    let mut supplied = BTreeMap::new();
    for decision in decisions {
        if supplied
            .insert(decision.proposal_id.as_str(), decision)
            .is_some()
        {
            return Err(StorageError::InvalidArgument(
                "memory proposal response contains duplicate decisions".into(),
            ));
        }
    }
    for (row, plan) in blockers.iter().zip(&continuation.calls) {
        let id: String = row.try_get("", "blocker_id")?;
        if id != plan.proposal_id || !supplied.contains_key(id.as_str()) {
            return Err(StorageError::InvalidArgument(
                "memory proposal response does not match blockers".into(),
            ));
        }
        let proposal = load_proposal(connection, &id).await?;
        if !matches!(
            proposal.status,
            MemoryProposalStatus::AwaitingConfirmation | MemoryProposalStatus::AwaitingReview
        ) || proposal.origin_run_id.as_deref() != Some(&context.run_id)
            || proposal.origin_node_instance_id.as_deref() != Some(&context.node_instance_id)
        {
            return Err(StorageError::Conflict("memory_proposal_projection"));
        }
        let linked = connection
            .query_one_raw(sql(
                "SELECT tool_call_id FROM memory_proposal_tool_calls WHERE proposal_id=?",
                vec![id.into()],
            ))
            .await?
            .ok_or_else(|| {
                StorageError::Integrity("memory proposal tool relation is missing".into())
            })?;
        if linked.try_get::<String>("", "tool_call_id")? != plan.tool_call_id {
            return Err(StorageError::Integrity(
                "memory proposal tool relation diverged".into(),
            ));
        }
    }
    Ok(())
}

async fn load_checkpoint<C: ConnectionTrait>(
    connection: &C,
    context: &WaitContext,
    wait_id: &str,
    continuation: &MemoryProposalContinuation,
) -> StorageResult<LlmLoopCheckpoint> {
    let row=connection.query_one_raw(sql("SELECT checkpoint_object_id,checkpoint_digest FROM llm_loop_checkpoints WHERE node_instance_id=?",vec![context.node_instance_id.clone().into()])).await?.ok_or_else(||StorageError::Integrity("memory proposal checkpoint is missing".into()))?;
    if row.try_get::<String>("", "checkpoint_object_id")? != continuation.checkpoint_ref
        || row.try_get::<String>("", "checkpoint_digest")? != continuation.checkpoint_digest
    {
        return Err(StorageError::Conflict("memory_proposal_checkpoint_changed"));
    }
    let checkpoint: LlmLoopCheckpoint =
        load_object_json(connection, &continuation.checkpoint_ref).await?;
    if !checkpoint.checksum_is_valid() || !checkpoint.wait_ids.iter().any(|id| id == wait_id) {
        return Err(StorageError::Integrity(
            "memory proposal checkpoint is invalid".into(),
        ));
    }
    Ok(checkpoint)
}

async fn persist_decision<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    decision: &MemoryProposalDecision,
    proposal: &zhuangsheng_core::memory::MemoryChangeProposalView,
    now: i64,
) -> StorageResult<String> {
    put_inline_object(connection,&canonical::to_vec(&json!({"schemaVersion":1,"proposalId":decision.proposal_id,"decision":decision.decision,"resultingStatus":proposal.status,"actorKind":command.actor_kind,"actorId":command.actor_id,"policyVersion":1}))?,now).await
}

async fn settle_blocker<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    decision: &MemoryProposalDecision,
    decision_ref: &str,
    now: i64,
) -> StorageResult<()> {
    let status = if decision.decision == ToolApprovalDecisionKind::Approve {
        "satisfied"
    } else {
        "rejected"
    };
    if connection.execute_raw(sql("UPDATE wait_blockers SET status=?,decision_object_id=? WHERE wait_id=? AND blocker_kind='memory_proposal' AND blocker_id=? AND status='open'",vec![status.into(),decision_ref.into(),command.wait_id.clone().into(),decision.proposal_id.clone().into()])).await?.rows_affected()!=1{return Err(StorageError::Conflict("memory_proposal_wait_blocker"));}
    add_object_ref(
        connection,
        decision_ref,
        "node_wait",
        &command.wait_id,
        "decision",
        now,
    )
    .await
}

async fn persist_tool_output<C: ConnectionTrait>(
    connection: &C,
    plan: &super::memory_proposal_tool::MemoryProposalCallPlan,
    proposal: &zhuangsheng_core::memory::MemoryChangeProposalView,
    now: i64,
) -> StorageResult<String> {
    let output = canonical::to_vec(
        &json!({"parts":[{"type":"llm_result","content":[{"type":"text","text":format!("Memory proposal {} is now {:?}.",proposal.id,proposal.status)}]},{"type":"memory_change_proposal","proposal":proposal}]}),
    )?;
    validate_tool_output(&output)?;
    let output_ref = put_inline_object(connection, &output, now).await?;
    if connection.execute_raw(sql("UPDATE tool_calls SET status='completed',output_object_id=?,finished_at=? WHERE id=? AND status='awaiting_approval'",vec![output_ref.clone().into(),now.into(),plan.tool_call_id.clone().into()])).await?.rows_affected()!=1{return Err(StorageError::Conflict("memory_proposal_tool_status"));}
    add_ref(
        connection,
        &output_ref,
        "tool_call",
        &plan.tool_call_id,
        "output",
        now,
    )
    .await?;
    Ok(output_ref)
}

async fn persist_response<C: ConnectionTrait>(
    connection: &C,
    command: &SubmitWaitResponseCommand,
    decision_refs: &[String],
    proposal_ids: &[String],
    now: i64,
) -> StorageResult<String> {
    let response_ref=put_inline_object(connection,&canonical::to_vec(&json!({"schemaVersion":1,"kind":"memory_proposal_review","decisionRefs":decision_refs,"proposalIds":proposal_ids}))?,now).await?;
    if connection.execute_raw(sql("UPDATE node_waits SET status='resolved',response_object_id=?,accepted_delivery_id=?,resolved_at=? WHERE id=? AND status='open'",vec![response_ref.clone().into(),command.delivery_id.clone().into(),now.into(),command.wait_id.clone().into()])).await?.rows_affected()!=1{return Err(StorageError::Conflict("memory_proposal_wait_settle"));}
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
