use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::EffectClassification,
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, EffectRetryPolicy, FinishModelCallCommand,
        LlmLogicalCallStatus, LlmLoopCheckpoint, ModelCallEffectOutcome,
        PrepareToolApprovalBatchCommand, PrepareToolApprovalCall, StartModelCallCommand,
        ToolCallCheckpoint, ToolCallCheckpointStatus,
    },
    runtime::{
        SubmitWaitResponseCommand, ToolApprovalDecision, ToolApprovalDecisionKind,
        WaitResponsePayload,
    },
    scheduler::ClaimedAttempt,
};

use crate::{
    SqliteStore,
    graph::helpers::{load_object_json, put_inline_object, sql},
    tests::{
        llm_ledger::{now_ms, prepare_command},
        llm_tool_support::prepare_running_tool_attempt,
        llm_tool_test_helpers::{
            DESCRIPTOR_DIGEST, IMPLEMENTATION_DIGEST, SCHEMA_DIGEST, digest, registry,
        },
    },
};

pub(super) struct ApprovalSetup {
    pub claimed: ClaimedAttempt,
    snapshot_ref: String,
    transcript_ref: String,
    model_response_ref: String,
    pub call_digest: String,
    pub now: i64,
}

pub(super) async fn prepare_model_tool_batch(store: &SqliteStore) -> ApprovalSetup {
    let claimed = prepare_running_tool_attempt(store).await;
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
                provider_request_id: Some("approval-model-request".into()),
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
                    response_bytes: canonical::to_vec(
                        &json!({"toolCalls":[{"name":"echo"},{"name":"echo"}]}),
                    )
                    .unwrap(),
                    usage: None,
                },
                checkpoint: model_checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Completed,
                ),
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
    let call_digest = digest(&snapshot.tools[0], json!({"text":"same"}));
    ApprovalSetup {
        claimed,
        snapshot_ref,
        transcript_ref,
        model_response_ref,
        call_digest,
        now,
    }
}

pub(super) fn approval_command(setup: &ApprovalSetup) -> PrepareToolApprovalBatchCommand {
    PrepareToolApprovalBatchCommand {
        wait_id: "approval-wait-1".into(),
        node_instance_id: setup.claimed.node_instance_id.clone(),
        originating_attempt_id: setup.claimed.attempt_id.clone(),
        model_call_id: "model-call-1".into(),
        calls: vec![
            approval_call(0, true, setup),
            approval_call(1, false, setup),
        ],
        checkpoint: approval_checkpoint(setup),
    }
}

fn approval_call(
    index: u64,
    requires_approval: bool,
    setup: &ApprovalSetup,
) -> PrepareToolApprovalCall {
    PrepareToolApprovalCall {
        tool_call_id: format!("tool-call-{}", index + 1),
        effect_id: format!("tool-effect-{}", index + 1),
        effect_attempt_id: format!("tool-effect-attempt-{}", index + 1),
        provider_call_id: Some(format!("provider-tool-call-{index}")),
        call_index: index,
        binding_id: "echo-binding".into(),
        tool_id: "echo-tool".into(),
        tool_version: "1".into(),
        call_digest: setup.call_digest.clone(),
        arguments_bytes: canonical::to_vec(&json!({"text":"same"})).unwrap(),
        descriptor_digest: DESCRIPTOR_DIGEST.into(),
        schema_compilation_digests: vec![SCHEMA_DIGEST.into()],
        implementation_digest: IMPLEMENTATION_DIGEST.into(),
        effect_classification: EffectClassification::Pure,
        effect_operation_key: "tool.echo".into(),
        descriptor_requires_approval: requires_approval,
        effect_idempotency_key: format!("tool-effect-{}:idempotency", index + 1),
        retry_policy: EffectRetryPolicy {
            max_attempts: 2,
            backoff_ms: vec![10],
        },
        risk_summary: if requires_approval {
            "Echo external input".into()
        } else {
            String::new()
        },
        approval_expires_at: setup.now + 60_000,
    }
}

pub(super) fn response_command(
    call_digest: &str,
    decision: ToolApprovalDecisionKind,
) -> SubmitWaitResponseCommand {
    SubmitWaitResponseCommand {
        wait_id: "approval-wait-1".into(),
        delivery_id: "approval-delivery-1".into(),
        actor_kind: "human".into(),
        actor_id: Some("user-1".into()),
        payload: WaitResponsePayload::ToolApproval {
            decisions: vec![ToolApprovalDecision {
                tool_call_id: "tool-call-1".into(),
                call_digest: call_digest.into(),
                decision,
                reason: Some("reviewed".into()),
            }],
        },
    }
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

fn approval_checkpoint(setup: &ApprovalSetup) -> LlmLoopCheckpoint {
    let calls = (0..2)
        .map(|index| ToolCallCheckpoint {
            tool_call_id: format!("tool-call-{}", index + 1),
            call_index: index,
            call_digest: setup.call_digest.clone(),
            status: if index == 0 {
                ToolCallCheckpointStatus::AwaitingApproval
            } else {
                ToolCallCheckpointStatus::Validated
            },
            effect_id: None,
            output_ref: None,
            wait_id: Some("approval-wait-1".into()),
        })
        .collect();
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
        current_batch: calls,
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used: 2,
        effect_watermark: "approval-wait-1".into(),
        wait_ids: vec!["approval-wait-1".into()],
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}

pub(super) async fn load_checkpoint(
    store: &SqliteStore,
    node_instance_id: &str,
) -> LlmLoopCheckpoint {
    let row = store
        .db
        .query_one(sql(
            "SELECT checkpoint_object_id FROM llm_loop_checkpoints WHERE node_instance_id = ?",
            vec![node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    load_object_json(
        &store.db,
        &row.try_get::<String>("", "checkpoint_object_id").unwrap(),
    )
    .await
    .unwrap()
}
