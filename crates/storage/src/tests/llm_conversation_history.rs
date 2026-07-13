use std::sync::Arc;

use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        context::CommitContextPatchCommand,
        conversation::{CreateConversationCommand, SubmitConversationTurnCommand},
        graph::CreateRolePlayTemplateCommand,
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    conversation::{ConversationInputShape, ConversationRunSpec},
    llm::{
        context::{
            ContextAssemblyMode, ContextAssemblySpec, ContextItem, ContextPosition, ContextRole,
            ContextSource, HistoryStrategy, OverflowPolicy, TokenBudgetHint,
        },
        ir::LlmContentPartIr,
    },
    scheduler::{Scheduler, SchedulerWork},
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{
    SqliteStore,
    tests::{llm_graph::channel_spec, store},
};

#[tokio::test]
async fn roleplay_attempt_pins_ordered_conversation_history_with_content() {
    let store = store().await;
    let channel = store
        .create_channel(CreateChannelCommand {
            name: "History channel".into(),
            idempotency_key: "history-channel".into(),
        })
        .await
        .unwrap();
    let mut channel_spec = channel_spec();
    channel_spec.model_catalogs[0].models[0]
        .capabilities
        .structured_output = Some(true);
    store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id.clone(),
            expected_head_revision_id: None,
            spec: channel_spec,
            idempotency_key: "history-channel-version".into(),
        })
        .await
        .unwrap();
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "History preset".into(),
            idempotency_key: "history-preset".into(),
        })
        .await
        .unwrap();
    store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: history_spec(),
            idempotency_key: "history-preset-version".into(),
        })
        .await
        .unwrap();
    let revision = store
        .create_roleplay_template(CreateRolePlayTemplateCommand {
            name: "History agent".into(),
            channel_id: channel.id,
            preset_id: preset.id,
            idempotency_key: "history-template".into(),
        })
        .await
        .unwrap();
    let run_spec = ConversationRunSpec {
        graph_revision_id: revision.id,
        reply_output_key: "reply".into(),
        input_shape: ConversationInputShape::ConversationMessageV1,
    };
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: Some(run_spec.clone()),
                idempotency_key: "history-conversation".into(),
            },
            1_700_001_000_000,
        )
        .await
        .unwrap();
    let submitted = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id,
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: vec![LlmContentPartIr::Text {
                    text: "Remember the moonlit archive".into(),
                }],
                run: run_spec,
                idempotency_key: "history-turn".into(),
            },
            1_700_001_000_001,
        )
        .await
        .unwrap();
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: submitted.run.context_id.clone(),
                lineage_key: submitted.run.branch_id.clone(),
                base_commit_id: submitted.run.input_commit_id.clone(),
                operation_id: "advance-after-run-start".into(),
                ops: vec![JsonPatchOp::Add {
                    path: "/runtimeMarker".into(),
                    value: serde_json::json!(true),
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::Application,
                    id: Some("history-test".into()),
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap();
    let claimed = claim_reply_attempt(&store, 1_700_001_000_002).await;
    let snapshot = claimed.context_snapshot.unwrap();
    let binding = snapshot.bindings.get("history").unwrap();
    assert_eq!(binding.version, submitted.run.input_commit_id);
    let [
        zhuangsheng_core::llm::context::ResolvedContextValue::HistoryMessage {
            message_id,
            stable_order,
            content,
            ..
        },
    ] = binding.values.as_slice()
    else {
        panic!("expected one history message")
    };
    assert_eq!(message_id, &submitted.turn.user_message_id);
    assert_eq!(*stable_order, 0);
    assert_eq!(
        content,
        &[LlmContentPartIr::Text {
            text: "Remember the moonlit archive".into(),
        }]
    );
}

fn history_spec() -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items: vec![ContextItem {
            id: "history".into(),
            name: None,
            enabled: true,
            requested_role: ContextRole::Context,
            source: ContextSource::History {
                binding_id: "history".into(),
                strategy: HistoryStrategy::All,
            },
            position: ContextPosition::History,
            order: 0,
            priority: 90,
            insertion_depth: 0,
            budget: TokenBudgetHint::default(),
            overflow: Some(OverflowPolicy::KeepRecent { count: None }),
        }],
        budget: None,
        post_process: vec![],
        text_transforms: vec![],
        preview: None,
    }
}

async fn claim_reply_attempt(
    store: &SqliteStore,
    now: i64,
) -> zhuangsheng_core::scheduler::ClaimedAttempt {
    Scheduler::new(Arc::new(store.clone()), "history-worker")
        .run_one(now)
        .await
        .unwrap();
    for offset in 1..=16 {
        let work = store
            .claim_next_work("history-worker", now + offset, now + offset + 30_000)
            .await
            .unwrap()
            .unwrap();
        match work {
            SchedulerWork::Attempt(attempt) if attempt.node.id == "reply" => return *attempt,
            SchedulerWork::Attempt(attempt) => panic!("unexpected attempt: {}", attempt.node.id),
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => store
                .activate_if_ready(&wakeup_id, &run_id, &node_id, now + offset)
                .await
                .unwrap(),
            SchedulerWork::Settle { wakeup_id, run_id } => store
                .settle_run(&wakeup_id, &run_id, now + offset)
                .await
                .unwrap(),
            SchedulerWork::Noop => {}
        }
    }
    panic!("role play LLM attempt was not scheduled")
}
