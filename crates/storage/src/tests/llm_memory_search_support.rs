use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::memory::{
        ApplyMemoryProposalCommand, DecideMemoryProposalCommand, MemoryProposalDecision,
        MemorySearchCommand, ProposeMemoryChangeCommand,
    },
    canonical,
    graph::{LlmMemoryBinding, MemoryToolCapability, MemoryToolGrant, NodeMemoryBinding},
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, ExecuteMemorySearchToolBatchCommand,
        FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint,
        MemorySearchToolCallCommand, MemorySearchToolCallDigestMaterial, ModelCallEffectOutcome,
        StartModelCallCommand, TOOL_CALL_POLICY_VERSION, ToolCallCheckpoint,
        ToolCallCheckpointStatus,
    },
    memory::{LongTermMemoryContentV1, MemoryProposalChangeInput, MemoryProposalStatus},
    scheduler::ClaimedAttempt,
    state::{ActorKind, ActorRef},
};

use crate::{
    SqliteStore,
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_ledger::{now_ms, prepare_command},
        llm_tool_support::prepare_running_tool_attempt_with_memory,
        llm_tool_test_helpers::registry,
    },
};

pub(super) struct MemorySearchSetup {
    pub claimed: ClaimedAttempt,
    snapshot_ref: String,
    transcript_ref: String,
    model_response_ref: String,
    call_digests: Vec<String>,
    queries: Vec<MemorySearchCommand>,
    pub now: i64,
}

pub(super) async fn prepare_memory_search_setup(store: &SqliteStore) -> MemorySearchSetup {
    add_memory_record(store, "dragon", "Dragons guard the northern gate").await;
    add_memory_record(store, "mage", "The mage studies the western tower").await;
    let memory = LlmMemoryBinding {
        node: NodeMemoryBinding::default(),
        tools: vec![MemoryToolGrant {
            capability: MemoryToolCapability::SearchMemory,
            scopes: vec!["roleplay".into()],
            max_results: Some(10),
            max_proposal_bytes: None,
        }],
    };
    let claimed = prepare_running_tool_attempt_with_memory(store, memory).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_ref: String = store
        .db
        .query_one(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "execution_snapshot_object_id")
        .unwrap();
    let transcript_ref = put_inline_object(
        &store.db,
        &canonical::to_vec(&json!({"schemaVersion":1,"items":[]})).unwrap(),
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
                model_checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Prepared,
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
                provider_request_id: Some("memory-search-model".into()),
                checkpoint: model_checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                ),
            },
            now + 1,
        )
        .await
        .unwrap();
    store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence,
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes: canonical::to_vec(&json!({
                        "toolCalls":[{"name":"search_memory"},{"name":"search_memory"}]
                    }))
                    .unwrap(),
                    usage: None,
                },
                checkpoint: model_checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Completed,
                ),
                transcript: None,
            },
            now + 2,
        )
        .await
        .unwrap();
    let model_response_ref: String = store
        .db
        .query_one(sql(
            "SELECT response_object_id FROM model_calls WHERE id = 'model-call-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "response_object_id")
        .unwrap();
    let queries = vec![
        MemorySearchCommand {
            scope_id: "roleplay".into(),
            text: Some("dragons northern".into()),
            tags: vec!["story".into()],
            status: None,
            limit: 10,
        },
        MemorySearchCommand {
            scope_id: "roleplay".into(),
            text: Some("missing phrase".into()),
            tags: vec![],
            status: None,
            limit: 10,
        },
    ];
    let grant = snapshot.memory.as_ref().unwrap().tools[0].clone();
    let call_digests = queries
        .iter()
        .map(|query| {
            MemorySearchToolCallDigestMaterial {
                query: query.clone(),
                grant: grant.clone(),
                policy_version: TOOL_CALL_POLICY_VERSION,
            }
            .digest()
            .unwrap()
        })
        .collect();
    MemorySearchSetup {
        claimed,
        snapshot_ref,
        transcript_ref,
        model_response_ref,
        call_digests,
        queries,
        now,
    }
}

pub(super) fn search_batch_command(
    setup: &MemorySearchSetup,
) -> ExecuteMemorySearchToolBatchCommand {
    let calls = setup
        .queries
        .iter()
        .enumerate()
        .map(|(index, query)| MemorySearchToolCallCommand {
            tool_call_id: format!("memory-search-call-{}", index + 1),
            provider_call_id: Some(format!("provider-memory-search-{index}")),
            call_index: index as u64,
            call_digest: setup.call_digests[index].clone(),
            query: query.clone(),
        })
        .collect::<Vec<_>>();
    ExecuteMemorySearchToolBatchCommand {
        node_instance_id: setup.claimed.node_instance_id.clone(),
        originating_attempt_id: setup.claimed.attempt_id.clone(),
        model_call_id: "model-call-1".into(),
        checkpoint: search_checkpoint(setup, &calls),
        calls,
    }
}

pub(super) async fn add_memory_record(store: &SqliteStore, key: &str, text: &str) {
    let proposal = store
        .propose_memory_change(ProposeMemoryChangeCommand {
            scope_id: "roleplay".into(),
            memory_id: None,
            expected_head_commit_id: None,
            change: MemoryProposalChangeInput::Create {
                content: LongTermMemoryContentV1 {
                    schema_version: 1,
                    text: text.into(),
                    tags: vec!["story".into()],
                    attributes: BTreeMap::new(),
                },
            },
            reason: "test memory".into(),
            evidence_refs: vec!["message:test".into()],
            requested_by: ActorRef {
                kind: ActorKind::User,
                id: Some("tester".into()),
            },
            idempotency_key: format!("propose-{key}"),
            schema_version: 1,
            policy_version: 1,
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap();
    store
        .decide_memory_proposal(DecideMemoryProposalCommand {
            proposal_id: proposal.id.clone(),
            expected_status: MemoryProposalStatus::AwaitingReview,
            decision: MemoryProposalDecision::Approve,
            actor: ActorRef {
                kind: ActorKind::User,
                id: Some("reviewer".into()),
            },
            idempotency_key: format!("approve-{key}"),
        })
        .await
        .unwrap();
    store
        .apply_memory_proposal(ApplyMemoryProposalCommand {
            proposal_id: proposal.id,
            expected_status: MemoryProposalStatus::Approved,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap();
}

fn model_checkpoint(
    claimed: &ClaimedAttempt,
    snapshot_ref: &str,
    transcript_ref: &str,
    status: LlmLogicalCallStatus,
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
        read_set_digest: canonical::hash(&json!({})).unwrap(),
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
        current_batch: vec![],
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used: 0,
        effect_watermark: "effect-attempt-1".into(),
        wait_ids: vec![],
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}

fn search_checkpoint(
    setup: &MemorySearchSetup,
    calls: &[MemorySearchToolCallCommand],
) -> LlmLoopCheckpoint {
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: setup.claimed.node_instance_id.clone(),
        last_updated_by_attempt_id: setup.claimed.attempt_id.clone(),
        graph_revision_id: setup
            .claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .graph_revision_id
            .clone(),
        registry_snapshot: registry(),
        context_snapshot_ref: setup.snapshot_ref.clone(),
        read_set_digest: canonical::hash(&json!({})).unwrap(),
        model_call_no: 1,
        transcript_ref: setup.transcript_ref.clone(),
        continuation_ref: None,
        active_model_effect: Some(ActiveModelEffectCheckpoint {
            model_call_id: "model-call-1".into(),
            effect_id: "effect-1".into(),
            status: LlmLogicalCallStatus::Completed,
            response_ref: Some(setup.model_response_ref.clone()),
        }),
        active_count_effect: None,
        current_batch: calls
            .iter()
            .map(|call| ToolCallCheckpoint {
                tool_call_id: call.tool_call_id.clone(),
                call_index: call.call_index,
                call_digest: call.call_digest.clone(),
                status: ToolCallCheckpointStatus::Completed,
                effect_id: None,
                output_ref: None,
                wait_id: None,
            })
            .collect(),
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used: calls.len() as u64,
        effect_watermark: calls.last().unwrap().tool_call_id.clone(),
        wait_ids: vec![],
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}
