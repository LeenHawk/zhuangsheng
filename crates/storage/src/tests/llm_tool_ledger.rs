use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::EffectClassification,
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, EffectRetryPolicy, FinishModelCallCommand,
        FinishToolCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint, ModelCallEffectOutcome,
        PrepareToolCallCommand, StartModelCallCommand, StartToolCallCommand, ToolCallCheckpoint,
        ToolCallCheckpointStatus, ToolCallOutcome,
    },
};

use crate::{
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_ledger::{now_ms, prepare_command},
        llm_tool_support::prepare_running_tool_attempt,
        llm_tool_test_helpers::{digest, registry, resolved, tool_checkpoint_call},
        store,
    },
};

#[tokio::test]
async fn executable_tool_calls_are_fenced_replayed_and_not_digest_deduplicated() {
    let store = store().await;
    let claimed = prepare_running_tool_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_ref = store
        .db
        .query_one(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "execution_snapshot_object_id")
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
                    None,
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
                provider_request_id: Some("tool-model-request".into()),
                checkpoint: model_checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
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
                fence: fence.clone(),
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes: canonical::to_vec(
                        &json!({"toolCalls":[{"name":"echo","arguments":{"text":"same"}}]}),
                    )
                    .unwrap(),
                    usage: None,
                },
                checkpoint: model_checkpoint(
                    &claimed,
                    &snapshot_ref,
                    &transcript_ref,
                    LlmLogicalCallStatus::Completed,
                    None,
                ),
                transcript: None,
            },
            now + 2,
        )
        .await
        .unwrap();
    let model_response_ref = store
        .db
        .query_one(sql(
            "SELECT response_object_id FROM model_calls WHERE id = 'model-call-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "response_object_id")
        .unwrap();
    let arguments = json!({"text":"same"});
    let arguments_bytes = canonical::to_vec(&arguments).unwrap();
    let call_digest = digest(&snapshot.tools[0], arguments.clone());
    let first_prepared = tool_checkpoint(
        &claimed,
        &snapshot_ref,
        &transcript_ref,
        &model_response_ref,
        vec![tool_checkpoint_call(
            "tool-call-1",
            "tool-effect-1",
            0,
            &call_digest,
            ToolCallCheckpointStatus::Prepared,
            None,
        )],
        "tool-effect-attempt-1",
        1,
    );
    let first = store
        .prepare_tool_call(
            tool_command(
                &claimed,
                0,
                "tool-call-1",
                "tool-effect-1",
                "tool-effect-attempt-1",
                &call_digest,
                arguments_bytes.clone(),
                first_prepared.clone(),
            ),
            now + 3,
        )
        .await
        .unwrap();
    assert!(!first.replayed);
    let replayed = store
        .prepare_tool_call(
            tool_command(
                &claimed,
                0,
                "tool-call-1",
                "tool-effect-1",
                "tool-effect-attempt-1",
                &call_digest,
                arguments_bytes.clone(),
                first_prepared.clone(),
            ),
            now + 4,
        )
        .await
        .unwrap();
    assert!(replayed.replayed);
    let first_running = tool_checkpoint(
        &claimed,
        &snapshot_ref,
        &transcript_ref,
        &model_response_ref,
        vec![tool_checkpoint_call(
            "tool-call-1",
            "tool-effect-1",
            0,
            &call_digest,
            ToolCallCheckpointStatus::Running,
            None,
        )],
        "tool-effect-attempt-1",
        1,
    );
    store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("tool-invocation-1".into()),
                checkpoint: first_running.clone(),
            },
            now + 5,
        )
        .await
        .unwrap();
    store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("tool-invocation-1".into()),
                checkpoint: first_running,
            },
            now + 6,
        )
        .await
        .unwrap();
    let output = canonical::to_vec(&json!({
        "parts":[{"type":"llm_result","content":[{"type":"text","text":"same"}]}]
    }))
    .unwrap();
    let first_completed = tool_checkpoint(
        &claimed,
        &snapshot_ref,
        &transcript_ref,
        &model_response_ref,
        vec![tool_checkpoint_call(
            "tool-call-1",
            "tool-effect-1",
            0,
            &call_digest,
            ToolCallCheckpointStatus::Completed,
            None,
        )],
        "tool-effect-attempt-1",
        1,
    );
    store
        .finish_tool_call(
            FinishToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-1".into(),
                fence: fence.clone(),
                outcome: ToolCallOutcome::Completed {
                    output_bytes: output.clone(),
                },
                checkpoint: first_completed.clone(),
            },
            now + 7,
        )
        .await
        .unwrap();
    store
        .finish_tool_call(
            FinishToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-1".into(),
                fence: fence.clone(),
                outcome: ToolCallOutcome::Completed {
                    output_bytes: output,
                },
                checkpoint: first_completed,
            },
            now + 8,
        )
        .await
        .unwrap();
    let first_output_ref = store
        .db
        .query_one(sql(
            "SELECT output_object_id FROM tool_calls WHERE id = 'tool-call-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "output_object_id")
        .unwrap();

    let second_checkpoint = tool_checkpoint(
        &claimed,
        &snapshot_ref,
        &transcript_ref,
        &model_response_ref,
        vec![
            tool_checkpoint_call(
                "tool-call-1",
                "tool-effect-1",
                0,
                &call_digest,
                ToolCallCheckpointStatus::Completed,
                Some(first_output_ref),
            ),
            tool_checkpoint_call(
                "tool-call-2",
                "tool-effect-2",
                1,
                &call_digest,
                ToolCallCheckpointStatus::Prepared,
                None,
            ),
        ],
        "tool-effect-attempt-2",
        2,
    );
    store
        .prepare_tool_call(
            tool_command(
                &claimed,
                1,
                "tool-call-2",
                "tool-effect-2",
                "tool-effect-attempt-2",
                &call_digest,
                arguments_bytes,
                second_checkpoint,
            ),
            now + 9,
        )
        .await
        .unwrap();
    let rows = store
        .db
        .query_all(sql(
            "SELECT id, call_index, call_digest FROM tool_calls ORDER BY call_index",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].try_get::<String>("", "call_digest").unwrap(),
        call_digest
    );
    assert_eq!(
        rows[1].try_get::<String>("", "call_digest").unwrap(),
        call_digest
    );
    assert_ne!(
        rows[0].try_get::<String>("", "id").unwrap(),
        rows[1].try_get::<String>("", "id").unwrap()
    );
    assert_eq!(rows[0].try_get::<i64>("", "call_index").unwrap(), 0);
    assert_eq!(rows[1].try_get::<i64>("", "call_index").unwrap(), 1);
    let events: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM run_events WHERE event_type LIKE 'llm.tool.%'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(events, 4);
}

#[allow(clippy::too_many_arguments)]
fn tool_command(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    call_index: u64,
    tool_call_id: &str,
    effect_id: &str,
    effect_attempt_id: &str,
    call_digest: &str,
    arguments_bytes: Vec<u8>,
    checkpoint: LlmLoopCheckpoint,
) -> PrepareToolCallCommand {
    let pins = resolved();
    PrepareToolCallCommand {
        tool_call_id: tool_call_id.into(),
        effect_id: effect_id.into(),
        effect_attempt_id: effect_attempt_id.into(),
        node_instance_id: claimed.node_instance_id.clone(),
        originating_attempt_id: claimed.attempt_id.clone(),
        model_call_id: "model-call-1".into(),
        provider_call_id: Some(format!("provider-tool-call-{call_index}")),
        call_index,
        binding_id: "echo-binding".into(),
        tool_id: "echo-tool".into(),
        tool_version: "1".into(),
        call_digest: call_digest.into(),
        arguments_bytes,
        descriptor_digest: pins.descriptor_digest,
        schema_compilation_digests: pins.schema_compilation_digests,
        implementation_digest: pins.implementation_digest,
        effect_classification: EffectClassification::Pure,
        effect_operation_key: "tool.echo".into(),
        descriptor_requires_approval: false,
        effect_idempotency_key: format!("{effect_id}:idempotency"),
        retry_policy: EffectRetryPolicy {
            max_attempts: 2,
            backoff_ms: vec![10],
        },
        checkpoint,
    }
}

#[allow(clippy::too_many_arguments)]
fn model_checkpoint(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    snapshot_ref: &str,
    transcript_ref: &str,
    status: LlmLogicalCallStatus,
    response_ref: Option<String>,
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
            response_ref,
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

#[allow(clippy::too_many_arguments)]
fn tool_checkpoint(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    snapshot_ref: &str,
    transcript_ref: &str,
    model_response_ref: &str,
    current_batch: Vec<ToolCallCheckpoint>,
    effect_attempt_id: &str,
    tool_calls_used: u64,
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
            status: LlmLogicalCallStatus::Completed,
            response_ref: Some(model_response_ref.into()),
        }),
        active_count_effect: None,
        current_batch,
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used,
        effect_watermark: effect_attempt_id.into(),
        wait_ids: vec![],
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}
