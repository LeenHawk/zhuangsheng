use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::memory::{DecideMemoryProposalCommand, MemoryProposalDecision as DomainDecision},
    graph::{LlmMemoryBinding, MemoryToolCapability, MemoryToolGrant, NodeMemoryBinding},
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, FinishModelCallCommand,
        LlmLogicalCallStatus, LlmLoopCheckpoint, MemoryProposalToolCallCommand,
        MemoryProposalToolCallDigestMaterial, MemoryProposalToolInput, ModelCallEffectOutcome,
        PrepareMemoryProposalToolBatchCommand, StartModelCallCommand, TOOL_CALL_POLICY_VERSION,
        ToolCallCheckpoint, ToolCallCheckpointStatus,
    },
    memory::{LongTermMemoryContentV1, MemoryProposalChangeInput, MemoryProposalStatus},
    runtime::{
        MemoryProposalDecision, RunControlCommand, SubmitWaitResponseCommand,
        ToolApprovalDecisionKind, WaitResponsePayload,
    },
};

use crate::{
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_ledger::{now_ms, prepare_command},
        llm_tool_support::prepare_running_tool_attempt_with_memory,
        llm_tool_test_helpers::registry,
        store,
    },
};

#[tokio::test]
async fn memory_proposal_batch_waits_decides_atomically_and_replays_delivery() {
    let (store, claimed, now, wait_id, proposal_ids) = prepare_proposal_wait().await;
    assert_eq!(proposal_ids.len(), 2);
    assert!(matches!(
        store
            .decide_memory_proposal(DecideMemoryProposalCommand {
                proposal_id: proposal_ids[0].clone(),
                expected_status: MemoryProposalStatus::AwaitingReview,
                decision: DomainDecision::Approve,
                actor: zhuangsheng_core::state::ActorRef {
                    kind: zhuangsheng_core::state::ActorKind::User,
                    id: Some("reviewer".into())
                },
                idempotency_key: "wrong-direct-decision".into(),
            })
            .await
            .unwrap_err(),
        crate::StorageError::Conflict("memory_proposal_wait_required")
    ));
    let incomplete = SubmitWaitResponseCommand {
        wait_id: wait_id.clone(),
        delivery_id: "memory-decision-incomplete".into(),
        actor_kind: "human".into(),
        actor_id: Some("reviewer".into()),
        payload: WaitResponsePayload::MemoryProposal {
            decisions: vec![MemoryProposalDecision {
                proposal_id: proposal_ids[0].clone(),
                decision: ToolApprovalDecisionKind::Approve,
            }],
        },
    };
    assert!(
        store
            .submit_wait_response(incomplete, now + 4)
            .await
            .is_err()
    );
    let open: i64 = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM wait_blockers WHERE wait_id=? AND status='open'",
            vec![wait_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(open, 2);
    let response = SubmitWaitResponseCommand {
        wait_id: wait_id.clone(),
        delivery_id: "memory-decision-1".into(),
        actor_kind: "human".into(),
        actor_id: Some("reviewer".into()),
        payload: WaitResponsePayload::MemoryProposal {
            decisions: vec![
                MemoryProposalDecision {
                    proposal_id: proposal_ids[0].clone(),
                    decision: ToolApprovalDecisionKind::Approve,
                },
                MemoryProposalDecision {
                    proposal_id: proposal_ids[1].clone(),
                    decision: ToolApprovalDecisionKind::Reject,
                },
            ],
        },
    };
    let resolved = store
        .submit_wait_response(response.clone(), now + 5)
        .await
        .unwrap();
    assert_eq!(resolved.decided_memory_proposal_ids, proposal_ids);
    assert!(resolved.prepared_tool_call_ids.is_empty());
    assert!(
        store
            .submit_wait_response(response, now + 6)
            .await
            .unwrap()
            .replayed
    );
    let rows = store
        .db
        .query_all_raw(sql(
            "SELECT status FROM memory_change_proposals ORDER BY id",
            vec![],
        ))
        .await
        .unwrap();
    let mut statuses: Vec<_> = rows
        .iter()
        .map(|row| row.try_get::<String>("", "status").unwrap())
        .collect();
    statuses.sort();
    assert_eq!(statuses, vec!["approved", "rejected"]);
    let proposal_events: Vec<String> = store
        .db
        .query_all_raw(sql(
            "SELECT event_type FROM run_events WHERE node_instance_id=? AND event_type LIKE 'memory.proposal.%' ORDER BY seq",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "event_type").unwrap())
        .collect();
    assert_eq!(
        proposal_events,
        [
            "memory.proposal.created",
            "memory.proposal.created",
            "memory.proposal.status_changed",
            "memory.proposal.status_changed",
        ]
    );
    let checkpoint: LlmLoopCheckpoint = load_checkpoint(&store, &claimed.node_instance_id).await;
    assert!(
        checkpoint
            .current_batch
            .iter()
            .all(|call| call.status == ToolCallCheckpointStatus::Completed
                && call.output_ref.is_some()
                && call.wait_id.is_none())
    );
    assert!(!checkpoint.wait_ids.contains(&wait_id));
}

#[tokio::test]
async fn terminal_run_aborts_memory_proposal_blockers_before_start() {
    let (store, claimed, _now, wait_id, _proposal_ids) = prepare_proposal_wait().await;
    let run_id: String = store
        .db
        .query_one_raw(sql(
            "SELECT run_id FROM node_instances WHERE id=?",
            vec![claimed.node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "run_id")
        .unwrap();
    store
        .request_cancel(RunControlCommand {
            run_id,
            expected_epoch: claimed.run_control_epoch,
            idempotency_key: "cancel-memory-proposal-wait".into(),
            reason: Some("test".into()),
        })
        .await
        .unwrap();
    let statuses: Vec<String> = store
        .db
        .query_all_raw(sql(
            "SELECT status FROM tool_calls ORDER BY call_index",
            vec![],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "status").unwrap())
        .collect();
    assert_eq!(statuses, vec!["cancelled_before_start"; 2]);
    let blockers: Vec<String> = store
        .db
        .query_all_raw(sql(
            "SELECT status FROM wait_blockers WHERE wait_id=? ORDER BY blocker_order",
            vec![wait_id.into()],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "status").unwrap())
        .collect();
    assert_eq!(blockers, vec!["aborted"; 2]);
}

async fn prepare_proposal_wait() -> (
    crate::SqliteStore,
    zhuangsheng_core::scheduler::ClaimedAttempt,
    i64,
    String,
    Vec<String>,
) {
    let store = store().await;
    let grant = MemoryToolGrant {
        capability: MemoryToolCapability::ProposeMemoryChange,
        scopes: vec!["roleplay".into()],
        max_results: None,
        max_proposal_bytes: Some(256 * 1024),
    };
    let memory = LlmMemoryBinding {
        node: NodeMemoryBinding::default(),
        tools: vec![grant.clone()],
    };
    let claimed = prepare_running_tool_attempt_with_memory(&store, memory).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_ref: String = store
        .db
        .query_one_raw(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id=?",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "execution_snapshot_object_id")
        .unwrap();
    let transcript_ref = put_inline_object(
        &store.db,
        &zhuangsheng_core::canonical::to_vec(&json!({"schemaVersion":1,"items":[]})).unwrap(),
        now,
    )
    .await
    .unwrap();
    let fence = EffectAttemptFence {
        invoking_node_attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
    };
    store
        .prepare_model_call(
            prepare_command(
                &claimed,
                &snapshot,
                checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Prepared,
                    vec![],
                    0,
                    "effect-attempt-1",
                ),
            ),
            now,
        )
        .await
        .unwrap();
    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: None,
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    vec![],
                    0,
                    "effect-attempt-1",
                ),
            },
            now + 1,
        )
        .await
        .unwrap();
    store.finish_model_call(FinishModelCallCommand{effect_attempt_id:"effect-attempt-1".into(),fence,outcome:ModelCallEffectOutcome::Completed{response_bytes:zhuangsheng_core::canonical::to_vec(&json!({"toolCalls":[{"name":"propose_memory_change"},{"name":"propose_memory_change"}]})).unwrap(),usage:None},checkpoint:checkpoint(&claimed,&snapshot_ref,&transcript_ref,LlmLogicalCallStatus::Completed,vec![],0,"effect-attempt-1"),transcript:None},now+2).await.unwrap();
    let inputs = [proposal("first"), proposal("second")];
    let calls: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(index, input)| MemoryProposalToolCallCommand {
            tool_call_id: format!("proposal-call-{index}"),
            provider_call_id: None,
            call_index: index as u64,
            call_digest: MemoryProposalToolCallDigestMaterial {
                input: input.clone(),
                grant: grant.clone(),
                policy_version: TOOL_CALL_POLICY_VERSION,
            }
            .digest()
            .unwrap(),
            input: input.clone(),
        })
        .collect();
    let wait_id = "memory-proposal-wait".to_string();
    let batch_checkpoint = checkpoint(
        &claimed,
        &snapshot_ref,
        &transcript_ref,
        LlmLogicalCallStatus::Completed,
        calls
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
            .collect(),
        2,
        &wait_id,
    );
    let prepared = store
        .prepare_memory_proposal_tool_batch(
            PrepareMemoryProposalToolBatchCommand {
                wait_id: wait_id.clone(),
                node_instance_id: claimed.node_instance_id.clone(),
                originating_attempt_id: claimed.attempt_id.clone(),
                model_call_id: "model-call-1".into(),
                calls,
                checkpoint: batch_checkpoint,
            },
            now + 3,
        )
        .await
        .unwrap();
    (store, claimed, now, wait_id, prepared.proposal_ids)
}

fn proposal(text: &str) -> MemoryProposalToolInput {
    MemoryProposalToolInput {
        scope_id: "roleplay".into(),
        memory_id: None,
        expected_head_commit_id: None,
        change: MemoryProposalChangeInput::Create {
            content: LongTermMemoryContentV1 {
                schema_version: 1,
                text: text.into(),
                tags: vec!["fact".into()],
                attributes: BTreeMap::new(),
            },
        },
        reason: "model found durable evidence".into(),
        evidence_refs: vec![format!("message:{text}")],
    }
}

fn checkpoint(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    snapshot_ref: &str,
    transcript_ref: &str,
    status: LlmLogicalCallStatus,
    current_batch: Vec<ToolCallCheckpoint>,
    used: u64,
    watermark: &str,
) -> LlmLoopCheckpoint {
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: claimed.node_instance_id.clone(),
        last_updated_by_attempt_id: claimed.attempt_id.clone(),
        graph_revision_id: claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .graph_revision_id
            .clone(),
        registry_snapshot: registry(),
        context_snapshot_ref: snapshot_ref.into(),
        read_set_digest: zhuangsheng_core::canonical::hash(&json!({})).unwrap(),
        model_call_no: 1,
        transcript_ref: transcript_ref.into(),
        continuation_ref: None,
        active_model_effect: Some(ActiveModelEffectCheckpoint {
            model_call_id: "model-call-1".into(),
            effect_id: "effect-1".into(),
            status,
            response_ref: None,
        }),
        active_count_effect: None,
        current_batch,
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used: used,
        effect_watermark: watermark.into(),
        wait_ids: if watermark.starts_with("memory-proposal") {
            vec![watermark.into()]
        } else {
            vec![]
        },
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}

async fn load_checkpoint(store: &crate::SqliteStore, instance: &str) -> LlmLoopCheckpoint {
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id=?",
            vec![instance.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    crate::graph::helpers::load_object_json(
        &store.db,
        &row.try_get::<String>("", "checkpoint_object_id").unwrap(),
    )
    .await
    .unwrap()
}
