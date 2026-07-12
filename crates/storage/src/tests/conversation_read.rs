use zhuangsheng_core::{
    application::conversation::{
        CreateConversationCommand, RegenerateConversationCandidateCommand,
        SelectConversationCandidateCommand, SubmitConversationTurnCommand,
    },
    conversation::{ConversationMessageRole, TurnCandidateStatus},
    llm::ir::LlmContentPartIr,
};

use crate::tests::store;

use super::{
    conversation_projection_support::complete_with_reply,
    conversation_run_profile::{compatible_revision, run_spec},
};

const NOW: i64 = 1_700_000_850_000;

#[tokio::test]
async fn conversation_reads_follow_active_ancestry_and_keep_sibling_candidates() {
    let store = store().await;
    let revision = compatible_revision(&store, "conversation-read").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: Some("The Archive".into()),
                default_run: None,
                idempotency_key: "conversation-read-root".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    assert!(
        store
            .get_conversation_timeline_view(&conversation.id)
            .await
            .unwrap()
            .messages
            .is_empty()
    );
    let submitted = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id.clone(),
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: vec![LlmContentPartIr::Text {
                    text: "Open the archive.".into(),
                }],
                run: run_spec(&revision),
                idempotency_key: "conversation-read-turn".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    complete_with_reply(
        &store,
        &submitted.run.id,
        &submitted.turn.user_commit_id,
        NOW + 2,
    )
    .await;
    store
        .maintain_candidate_projections(NOW + 3, "conversation-read-first", 10)
        .await
        .unwrap();
    let regenerated = store
        .regenerate_conversation_candidate_at(
            RegenerateConversationCandidateCommand {
                turn_id: submitted.turn.id.clone(),
                expected_user_commit_id: submitted.turn.user_commit_id.clone(),
                run: run_spec(&revision),
                idempotency_key: "conversation-read-regenerate".into(),
            },
            NOW + 4,
        )
        .await
        .unwrap();
    complete_with_reply(
        &store,
        &regenerated.run.id,
        &submitted.turn.user_commit_id,
        NOW + 5,
    )
    .await;
    store
        .maintain_candidate_projections(NOW + 6, "conversation-read-second", 10)
        .await
        .unwrap();
    let first_timeline = store
        .get_conversation_timeline_view(&conversation.id)
        .await
        .unwrap();
    assert_eq!(first_timeline.messages.len(), 2);
    assert_eq!(first_timeline.turns.len(), 1);
    assert_eq!(first_timeline.turns[0].candidates.len(), 2);
    assert_eq!(
        first_timeline.messages[1].origin_run_id.as_deref(),
        Some(submitted.run.id.as_str())
    );
    let second = first_timeline.turns[0]
        .candidates
        .iter()
        .find(|candidate| candidate.run_id == regenerated.run.id)
        .unwrap();
    assert_eq!(second.status, TurnCandidateStatus::Ready);
    let second_commit = second.candidate_commit_id.clone().unwrap();
    store
        .select_conversation_candidate_at(
            SelectConversationCandidateCommand {
                turn_id: submitted.turn.id,
                selected_run_id: regenerated.run.id.clone(),
                expected_conversation_head_commit_id: first_timeline.active_head_commit_id,
                idempotency_key: "conversation-read-select".into(),
            },
            NOW + 7,
        )
        .await
        .unwrap();
    let selected = store
        .get_conversation_timeline_view(&conversation.id)
        .await
        .unwrap();
    assert_eq!(selected.active_head_commit_id, second_commit);
    assert_eq!(
        selected.turns[0].selected_run_id,
        Some(regenerated.run.id.clone())
    );
    assert_eq!(selected.messages[1].origin_run_id, Some(regenerated.run.id));
    assert_eq!(selected.messages[0].role, ConversationMessageRole::User);
    assert_eq!(
        store.list_conversation_views().await.unwrap().items[0].id,
        conversation.id
    );
}
