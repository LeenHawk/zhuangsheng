use std::sync::Arc;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    canonical,
    graph::EffectClassification,
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, EffectRetryPolicy, FinishModelCallCommand,
        LlmLogicalCallStatus, LlmLoopCheckpoint, ModelCallEffectOutcome, PrepareModelCallCommand,
        StartModelCallCommand,
        context::{ContextAssemblyMode, ContextAssemblySpec},
        ir::LlmUsageIr,
    },
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::{ClaimedAttempt, Scheduler, SchedulerWork},
};

use crate::{
    SqliteStore, StorageError,
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_graph::{channel_spec, llm_draft},
        store,
    },
};

#[tokio::test]
async fn model_effect_ledger_is_fenced_idempotent_and_terminal() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_object_id = store
        .db
        .query_one_raw(sql(
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
    let prepared_checkpoint = checkpoint(
        &claimed,
        &snapshot_object_id,
        &transcript_ref,
        LlmLogicalCallStatus::Prepared,
        None,
    );
    let mut stale_prepare = prepare_command(&claimed, &snapshot, prepared_checkpoint.clone());
    stale_prepare.fence.worker_id = "stale-worker".into();
    assert!(matches!(
        store.prepare_model_call(stale_prepare, now).await,
        Err(StorageError::Conflict("effect_attempt_fence"))
    ));
    let prepared = store
        .prepare_model_call(
            prepare_command(&claimed, &snapshot, prepared_checkpoint.clone()),
            now,
        )
        .await;
    let prepared = prepared.unwrap();
    assert!(!prepared.replayed);

    let replayed = store
        .prepare_model_call(
            prepare_command(&claimed, &snapshot, prepared_checkpoint.clone()),
            now + 1,
        )
        .await
        .unwrap();
    assert!(replayed.replayed);

    let fence = EffectAttemptFence {
        invoking_node_attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
    };
    let wrong = store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: EffectAttemptFence {
                    worker_id: "wrong-worker".into(),
                    ..fence.clone()
                },
                provider_request_id: Some("provider-request-1".into()),
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
                ),
            },
            now + 2,
        )
        .await;
    assert!(matches!(
        wrong,
        Err(StorageError::Conflict("effect_attempt_fence"))
    ));

    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("provider-request-1".into()),
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
                ),
            },
            now + 3,
        )
        .await
        .unwrap();

    let running_checkpoint = checkpoint(
        &claimed,
        &snapshot_object_id,
        &transcript_ref,
        LlmLogicalCallStatus::Running,
        None,
    );
    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("provider-request-1".into()),
                checkpoint: running_checkpoint.clone(),
            },
            now + 4,
        )
        .await
        .unwrap();
    let conflicting_start = store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("different-provider-request".into()),
                checkpoint: running_checkpoint,
            },
            now + 4,
        )
        .await;
    assert!(matches!(
        conflicting_start,
        Err(StorageError::Conflict("model_call_start_replay"))
    ));

    let response_bytes = canonical::to_vec(&json!({"text":"hello"})).unwrap();
    let completed_checkpoint = checkpoint(
        &claimed,
        &snapshot_object_id,
        &transcript_ref,
        LlmLogicalCallStatus::Completed,
        None,
    );
    store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes: response_bytes.clone(),
                    usage: Some(LlmUsageIr {
                        input_tokens: Some(10),
                        output_tokens: Some(5),
                        total_tokens: Some(15),
                        cached_input_tokens: None,
                        reasoning_tokens: None,
                    }),
                },
                checkpoint: completed_checkpoint.clone(),
                transcript: None,
            },
            now + 5,
        )
        .await
        .unwrap();
    store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes,
                    usage: Some(LlmUsageIr {
                        input_tokens: Some(10),
                        output_tokens: Some(5),
                        total_tokens: Some(15),
                        cached_input_tokens: None,
                        reasoning_tokens: None,
                    }),
                },
                checkpoint: completed_checkpoint.clone(),
                transcript: None,
            },
            now + 6,
        )
        .await
        .unwrap();
    let conflicting_finish = store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence,
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes: canonical::to_vec(&json!({"text":"different"})).unwrap(),
                    usage: Some(LlmUsageIr {
                        input_tokens: Some(10),
                        output_tokens: Some(5),
                        total_tokens: Some(15),
                        cached_input_tokens: None,
                        reasoning_tokens: None,
                    }),
                },
                checkpoint: completed_checkpoint,
                transcript: None,
            },
            now + 7,
        )
        .await;
    assert!(matches!(
        conflicting_finish,
        Err(StorageError::Conflict("model_call_finish_replay"))
    ));
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT mc.status AS model_status, e.status AS effect_status, ea.status AS attempt_status, mc.response_object_id FROM model_calls mc JOIN effects e ON e.model_call_id = mc.id JOIN effect_attempts ea ON ea.effect_id = e.id WHERE mc.id = 'model-call-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "model_status").unwrap(),
        "completed"
    );
    assert_eq!(
        row.try_get::<String>("", "effect_status").unwrap(),
        "succeeded"
    );
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        "succeeded"
    );
    assert!(
        !row.try_get::<String>("", "response_object_id")
            .unwrap()
            .is_empty()
    );
    let events: Vec<String> = store
        .db
        .query_all_raw(sql(
            "SELECT event_type FROM run_events WHERE node_instance_id = ? AND event_type IN ('effect.prepared','effect.started','effect.succeeded','llm.call.started','llm.call.completed') ORDER BY seq",
            vec![claimed.node_instance_id.into()],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "event_type").unwrap())
        .collect();
    assert_eq!(
        events,
        [
            "effect.prepared",
            "effect.started",
            "llm.call.started",
            "effect.succeeded",
            "llm.call.completed",
        ]
    );
}

pub(super) fn prepare_command(
    claimed: &ClaimedAttempt,
    snapshot: &zhuangsheng_core::graph::LlmNodeExecutionSnapshot,
    checkpoint: LlmLoopCheckpoint,
) -> PrepareModelCallCommand {
    PrepareModelCallCommand {
        model_call_id: "model-call-1".into(),
        effect_id: "effect-1".into(),
        effect_attempt_id: "effect-attempt-1".into(),
        node_instance_id: claimed.node_instance_id.clone(),
        originating_attempt_id: claimed.attempt_id.clone(),
        fence: EffectAttemptFence {
            invoking_node_attempt_id: claimed.attempt_id.clone(),
            worker_id: claimed.worker_id.clone(),
            lease_fence: claimed.lease_fence,
            run_control_epoch: claimed.run_control_epoch,
        },
        call_no: 1,
        channel_id: snapshot.channel.channel_id.clone(),
        operation: snapshot.operation.clone(),
        request_bytes: canonical::to_vec(&json!({"model":"roleplay-model","input":"hi"})).unwrap(),
        effect_kind: "model_generation".into(),
        effect_classification: EffectClassification::Pure,
        effect_operation_key: "llm.generate".into(),
        effect_idempotency_key: format!("model-effect:{}:1", claimed.node_instance_id),
        retry_policy: EffectRetryPolicy {
            max_attempts: 2,
            backoff_ms: vec![100],
        },
        checkpoint,
    }
}

pub(super) fn checkpoint(
    claimed: &ClaimedAttempt,
    snapshot_object_id: &str,
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
        registry_snapshot: claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .tool_registry
            .clone(),
        context_snapshot_ref: snapshot_object_id.into(),
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
        current_batch: Vec::new(),
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used: 0,
        effect_watermark: "effect-attempt-1".into(),
        wait_ids: Vec::new(),
        checksum: String::new(),
    }
    .seal()
    .unwrap()
}

pub(super) async fn prepare_running_llm_attempt(store: &SqliteStore) -> ClaimedAttempt {
    let channel = store
        .create_channel(CreateChannelCommand {
            name: "LLM".into(),
            idempotency_key: "ledger-channel".into(),
        })
        .await
        .unwrap();
    store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id.clone(),
            expected_head_revision_id: None,
            spec: channel_spec(),
            idempotency_key: "ledger-channel-revision".into(),
        })
        .await
        .unwrap();
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "RP".into(),
            idempotency_key: "ledger-preset".into(),
        })
        .await
        .unwrap();
    store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: ContextAssemblySpec {
                id: None,
                name: None,
                mode: ContextAssemblyMode::Chat,
                items: vec![],
                budget: None,
                post_process: vec![],
                text_transforms: vec![],
                text_transform_macros: Default::default(),
                preview: None,
            },
            idempotency_key: "ledger-preset-version".into(),
        })
        .await
        .unwrap();
    let graph = store
        .create_graph(CreateGraphCommand {
            name: "Ledger Graph".into(),
            idempotency_key: "ledger-graph".into(),
        })
        .await
        .unwrap();
    let draft = store.get_graph_draft(&graph.graph.id).await.unwrap();
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.graph.id.clone(),
            expected_revision_token: draft.revision_token,
            document: llm_draft(&graph.graph.id, &channel.id, &preset.id),
            idempotency_key: "ledger-graph-draft".into(),
        })
        .await
        .unwrap();
    let revision = store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: "ledger-graph-apply".into(),
        })
        .await
        .unwrap();
    store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "ledger-run".into(),
        })
        .await
        .unwrap();
    let now = now_ms();
    Scheduler::new(Arc::new(store.clone()), "ledger-worker")
        .run_one(now)
        .await
        .unwrap();
    for _ in 0..16 {
        let work = store
            .claim_next_work("ledger-worker", now + 1, now + 30_000)
            .await
            .unwrap()
            .unwrap();
        match work {
            SchedulerWork::Attempt(attempt) => {
                assert_eq!(attempt.node.id, "generate");
                store.mark_attempt_running(&attempt, now + 1).await.unwrap();
                return *attempt;
            }
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => store
                .activate_if_ready(&wakeup_id, &run_id, &node_id, now + 1)
                .await
                .unwrap(),
            SchedulerWork::Settle { wakeup_id, run_id } => store
                .settle_run(&wakeup_id, &run_id, now + 1)
                .await
                .unwrap(),
            SchedulerWork::Noop => {}
        }
    }
    panic!("LLM attempt was not scheduled")
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
