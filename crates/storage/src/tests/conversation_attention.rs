use serde_json::json;
use zhuangsheng_core::{
    application::conversation::{CreateConversationCommand, SubmitConversationTurnCommand},
    conversation::ConversationAttentionKind,
    llm::ir::LlmContentPartIr,
    runtime::WaitKind,
    scheduler::{
        BuiltinResult, ExternalWaitRequest, FinalizeAttemptCommand, SchedulerWork,
        WaitTimeoutPolicy,
    },
};

use super::{
    conversation_run_profile::{compatible_revision, run_spec},
    schema, store,
};

const NOW: i64 = 1_700_000_850_000;

#[tokio::test]
async fn conversation_list_projects_open_attention_in_one_query_surface() {
    let store = store().await;
    let revision = compatible_revision(&store, "conversation-attention").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: Some("Needs a choice".into()),
                default_run: None,
                idempotency_key: "conversation-attention-root".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    let submitted = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id.clone(),
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: vec![LlmContentPartIr::Text {
                    text: "Which door?".into(),
                }],
                run: run_spec(&revision),
                idempotency_key: "conversation-attention-turn".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    let SchedulerWork::Attempt(attempt) = store
        .claim_next_work("attention-worker", NOW + 2, NOW + 30_000)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("expected conversation input attempt")
    };
    store.mark_attempt_running(&attempt, NOW + 2).await.unwrap();
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: attempt.wakeup_id,
                attempt_id: attempt.attempt_id.clone(),
                worker_id: attempt.worker_id,
                lease_fence: attempt.lease_fence,
                run_control_epoch: attempt.run_control_epoch,
                result_idempotency_key: "conversation-attention-result".into(),
                result: BuiltinResult::Waiting {
                    wait: Box::new(ExternalWaitRequest {
                        kind: WaitKind::HumanResponse,
                        request: json!({"schemaVersion":1,"kind":"human_response","title":"Choose"}),
                        response_schema: Some(schema(json!({"type":"string"}))),
                        correlation_key: Some("conversation-attention-choice".into()),
                        deadline_at: None,
                        on_timeout: WaitTimeoutPolicy::Fail,
                    }),
                    continuation: json!({"schemaVersion":1,"step":"chosen"}),
                },
            },
            NOW + 3,
        )
        .await
        .unwrap();

    let list = store.list_conversation_views().await.unwrap();
    assert_eq!(list.attention.len(), 1);
    assert_eq!(list.attention[0].conversation_id, conversation.id);
    assert_eq!(list.attention[0].run_id, submitted.run.id);
    assert_eq!(
        list.attention[0].kind,
        ConversationAttentionKind::HumanResponse
    );
    assert!(list.attention[0].wait_id.is_some());
}
