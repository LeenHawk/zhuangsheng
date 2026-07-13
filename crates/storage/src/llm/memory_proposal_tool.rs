use sea_orm::{ConnectionTrait, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;
use zhuangsheng_core::{
    application::memory::ProposeMemoryChangeCommand,
    canonical,
    llm::{
        MEMORY_PROPOSAL_BINDING_ID, MEMORY_PROPOSAL_TOOL_ID, MEMORY_PROPOSAL_TOOL_VERSION,
        PrepareMemoryProposalToolBatchCommand, PreparedMemoryProposalToolBatch,
    },
    state::{ActorKind, ActorRef},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    memory::{load_proposal, propose_in},
    runtime::{Event, add_object_ref, append_event},
};

use super::{
    memory_proposal_tool_validation::validate_memory_proposal_batch,
    model_ledger_helpers::{add_ref, persist_checkpoint},
    validation::load_ledger_context,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemoryProposalContinuation {
    pub schema_version: u32,
    pub prepare_digest: String,
    pub node_instance_id: String,
    pub originating_attempt_id: String,
    pub model_call_id: String,
    pub checkpoint_ref: String,
    pub checkpoint_digest: String,
    pub calls: Vec<MemoryProposalCallPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MemoryProposalCallPlan {
    pub tool_call_id: String,
    pub proposal_id: String,
    pub call_index: u64,
    pub call_digest: String,
}

impl SqliteStore {
    pub async fn prepare_memory_proposal_tool_batch(
        &self,
        command: PrepareMemoryProposalToolBatchCommand,
        now: i64,
    ) -> StorageResult<PreparedMemoryProposalToolBatch> {
        let digest = prepare_digest(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(view) = replay(&transaction, &command, &digest).await? {
            transaction.commit().await?;
            return Ok(view);
        }
        let context = load_ledger_context(
            &transaction,
            &command.node_instance_id,
            &command.originating_attempt_id,
        )
        .await?;
        let (run_id, node_id) =
            validate_memory_proposal_batch(&transaction, &context, &command).await?;
        let mut plans = Vec::with_capacity(command.calls.len());
        for call in &command.calls {
            let arguments_ref =
                put_inline_object(&transaction, &canonical::to_vec(&call.input)?, now).await?;
            transaction.execute_raw(sql(
                "INSERT INTO tool_calls (id,node_instance_id,originating_attempt_id,model_call_id,provider_call_id,call_index,binding_id,tool_id,tool_version,call_digest,arguments_object_id,status,created_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,'awaiting_approval',?)",
                vec![call.tool_call_id.clone().into(),command.node_instance_id.clone().into(),command.originating_attempt_id.clone().into(),command.model_call_id.clone().into(),call.provider_call_id.clone().into(),i64::try_from(call.call_index).map_err(|_| StorageError::InvalidArgument("tool call index is too large".into()))?.into(),MEMORY_PROPOSAL_BINDING_ID.into(),MEMORY_PROPOSAL_TOOL_ID.into(),MEMORY_PROPOSAL_TOOL_VERSION.into(),call.call_digest.clone().into(),arguments_ref.clone().into(),now.into()],
            )).await?;
            add_ref(
                &transaction,
                &arguments_ref,
                "tool_call",
                &call.tool_call_id,
                "arguments",
                now,
            )
            .await?;
            let proposal = propose_in(
                &transaction,
                ProposeMemoryChangeCommand {
                    scope_id: call.input.scope_id.clone(),
                    memory_id: call.input.memory_id.clone(),
                    expected_head_commit_id: call.input.expected_head_commit_id.clone(),
                    change: call.input.change.clone(),
                    reason: call.input.reason.clone(),
                    evidence_refs: call.input.evidence_refs.clone(),
                    requested_by: ActorRef {
                        kind: ActorKind::Node,
                        id: Some(command.node_instance_id.clone()),
                    },
                    idempotency_key: format!("memory-tool:{}", call.tool_call_id),
                    schema_version: 1,
                    policy_version: 1,
                    origin_run_id: Some(run_id.clone()),
                    origin_node_instance_id: Some(command.node_instance_id.clone()),
                },
                now,
            )
            .await?;
            transaction.execute_raw(sql(
                "INSERT INTO memory_proposal_tool_calls (proposal_id,tool_call_id,created_at) VALUES (?,?,?)",
                vec![proposal.id.clone().into(),call.tool_call_id.clone().into(),now.into()],
            )).await?;
            plans.push(MemoryProposalCallPlan {
                tool_call_id: call.tool_call_id.clone(),
                proposal_id: proposal.id,
                call_index: call.call_index,
                call_digest: call.call_digest.clone(),
            });
        }
        persist_checkpoint(&transaction, &command.checkpoint, now).await?;
        open_wait(
            &transaction,
            &command,
            &digest,
            &run_id,
            &node_id,
            plans.clone(),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(PreparedMemoryProposalToolBatch {
            wait_id: command.wait_id,
            proposal_ids: plans.into_iter().map(|plan| plan.proposal_id).collect(),
            replayed: false,
        })
    }
}

async fn open_wait<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareMemoryProposalToolBatchCommand,
    digest: &str,
    run_id: &str,
    node_id: &str,
    plans: Vec<MemoryProposalCallPlan>,
    now: i64,
) -> StorageResult<()> {
    let checkpoint_ref: String = connection
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id=?",
            vec![command.node_instance_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("memory proposal checkpoint is missing".into()))?
        .try_get("", "checkpoint_object_id")?;
    let mut proposal_views = Vec::with_capacity(plans.len());
    for plan in &plans {
        proposal_views.push((plan, load_proposal(connection, &plan.proposal_id).await?));
    }
    let request_ref = put_inline_object(connection, &canonical::to_vec(&json!({"schemaVersion":1,"kind":"memory_proposal_review","modelCallId":command.model_call_id,"proposals":proposal_views.iter().map(|(plan, proposal)| json!({"proposalId":plan.proposal_id,"toolCallId":plan.tool_call_id,"proposal":proposal})).collect::<Vec<_>>() }))?, now).await?;
    let continuation = MemoryProposalContinuation {
        schema_version: 1,
        prepare_digest: digest.into(),
        node_instance_id: command.node_instance_id.clone(),
        originating_attempt_id: command.originating_attempt_id.clone(),
        model_call_id: command.model_call_id.clone(),
        checkpoint_ref: checkpoint_ref.clone(),
        checkpoint_digest: command.checkpoint.checksum.clone(),
        calls: plans.clone(),
    };
    let continuation_ref =
        put_inline_object(connection, &canonical::to_vec(&continuation)?, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO node_waits (id,run_id,node_instance_id,node_attempt_id,kind,correlation_key,request_object_id,continuation_object_id,on_timeout,status,created_at) VALUES (?,?,?,?,'approval',?,?,?,'fail','open',?)",
        vec![command.wait_id.clone().into(),run_id.into(),command.node_instance_id.clone().into(),command.originating_attempt_id.clone().into(),format!("memory-proposal:{}",command.model_call_id).into(),request_ref.clone().into(),continuation_ref.clone().into(),now.into()],
    )).await?;
    for plan in &plans {
        connection.execute_raw(sql("INSERT INTO wait_blockers (wait_id,blocker_kind,blocker_id,blocker_order,status) VALUES (?,'memory_proposal',?,?,'open')", vec![command.wait_id.clone().into(),plan.proposal_id.clone().into(),i64::try_from(plan.call_index).map_err(|_| StorageError::InvalidArgument("proposal order is too large".into()))?.into()])).await?;
    }
    transition_owner(connection, command, run_id, node_id, &continuation_ref, now).await?;
    for (object, role) in [
        (&request_ref, "request"),
        (&continuation_ref, "continuation"),
        (&checkpoint_ref, "checkpoint"),
    ] {
        add_object_ref(connection, object, "node_wait", &command.wait_id, role, now).await?;
    }
    append_event(connection, Event { run_id, event_type: "llm.tool.memory_proposals_requested", importance: "critical", node_instance_id: Some(&command.node_instance_id), attempt_id: Some(&command.originating_attempt_id), payload: json!({"schemaVersion":1,"waitId":command.wait_id,"modelCallId":command.model_call_id,"proposalIds":plans.iter().map(|plan| &plan.proposal_id).collect::<Vec<_>>() }), now }).await?;
    for plan in &plans {
        for event_type in ["tool.call.requested", "tool.call.awaiting_approval"] {
            append_event(connection, Event { run_id, event_type, importance: "critical", node_instance_id: Some(&command.node_instance_id), attempt_id: Some(&command.originating_attempt_id), payload: json!({"schemaVersion":1,"waitId":command.wait_id,"modelCallId":command.model_call_id,"toolCallId":plan.tool_call_id,"callIndex":plan.call_index,"proposalId":plan.proposal_id}), now }).await?;
        }
    }
    Ok(())
}

async fn transition_owner<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareMemoryProposalToolBatchCommand,
    run_id: &str,
    node_id: &str,
    continuation_ref: &str,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute_raw(sql("UPDATE node_attempts SET status='waiting',continuation_object_id=?,worker_id=NULL,lease_until=NULL,finished_at=? WHERE id=? AND node_instance_id=? AND status='running'", vec![continuation_ref.into(),now.into(),command.originating_attempt_id.clone().into(),command.node_instance_id.clone().into()])).await?;
    let instance = connection.execute_raw(sql("UPDATE node_instances SET status='waiting',updated_at=? WHERE id=? AND status='running'", vec![now.into(),command.node_instance_id.clone().into()])).await?;
    if attempt.rows_affected() != 1 || instance.rows_affected() != 1 {
        return Err(StorageError::Conflict("memory_proposal_wait_owner"));
    }
    connection.execute_raw(sql("UPDATE runtime_timers SET status='cancelled' WHERE node_attempt_id=? AND kind='attempt_deadline' AND status='pending'", vec![command.originating_attempt_id.clone().into()])).await?;
    connection.execute_raw(sql("UPDATE scheduler_wakeups SET status='done',claimed_by=NULL,lease_until=NULL WHERE run_id=? AND node_id=? AND kind='attempt_ready' AND status='claimed'", vec![run_id.into(),node_id.into()])).await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET open_waits=open_waits+1 WHERE run_id=?",
            vec![run_id.into()],
        ))
        .await?;
    let active = connection.query_one_raw(sql("SELECT 1 AS present FROM node_instances WHERE run_id=? AND status IN ('ready','running') LIMIT 1",vec![run_id.into()])).await?.is_some();
    let dispatch = connection.query_one_raw(sql("SELECT 1 AS present FROM scheduler_wakeups WHERE run_id=? AND kind IN ('node_maybe_ready','attempt_ready') AND status IN ('pending','claimed') LIMIT 1",vec![run_id.into()])).await?.is_some();
    if !active && !dispatch && connection.execute_raw(sql("UPDATE graph_runs SET status='waiting',updated_at=? WHERE id=? AND status='running'",vec![now.into(),run_id.into()])).await?.rows_affected()==1 {
        append_event(connection, Event { run_id, event_type:"run.waiting",importance:"critical",node_instance_id:None,attempt_id:None,payload:json!({"schemaVersion":1,"reason":"memory_proposal_review"}),now }).await?;
    }
    Ok(())
}

fn prepare_digest(command: &PrepareMemoryProposalToolBatchCommand) -> StorageResult<String> {
    canonical::hash(&json!({"schemaVersion":1,"waitId":command.wait_id,"nodeInstanceId":command.node_instance_id,"originatingAttemptId":command.originating_attempt_id,"modelCallId":command.model_call_id,"calls":command.calls.iter().map(|call| json!({"toolCallId":call.tool_call_id,"providerCallId":call.provider_call_id,"callIndex":call.call_index,"callDigest":call.call_digest,"input":call.input})).collect::<Vec<_>>(),"checkpointDigest":command.checkpoint.checksum})) .map_err(Into::into)
}

async fn replay<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareMemoryProposalToolBatchCommand,
    digest: &str,
) -> StorageResult<Option<PreparedMemoryProposalToolBatch>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT continuation_object_id FROM node_waits WHERE id=? AND node_instance_id=?",
            vec![
                command.wait_id.clone().into(),
                command.node_instance_id.clone().into(),
            ],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    let continuation: MemoryProposalContinuation = load_object_json(
        connection,
        &row.try_get::<String>("", "continuation_object_id")?,
    )
    .await?;
    if continuation.prepare_digest != digest
        || continuation.checkpoint_digest != command.checkpoint.checksum
    {
        return Err(StorageError::Conflict("memory_proposal_batch_replay"));
    }
    Ok(Some(PreparedMemoryProposalToolBatch {
        wait_id: command.wait_id.clone(),
        proposal_ids: continuation
            .calls
            .into_iter()
            .map(|plan| plan.proposal_id)
            .collect(),
        replayed: true,
    }))
}
