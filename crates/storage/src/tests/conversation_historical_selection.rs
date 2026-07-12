use zhuangsheng_core::{
    application::conversation::{
        CreateConversationCommand, SelectConversationCandidateCommand,
        SubmitConversationTurnCommand,
    },
    llm::ir::LlmContentPartIr,
};

use crate::tests::store;

use super::{
    conversation_projection_support::complete_with_reply,
    conversation_run_profile::{compatible_revision, run_spec},
};

const NOW: i64 = 1_700_000_510_000;

#[tokio::test]
async fn selecting_a_historical_candidate_forks_without_deleting_later_history() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "historical-selection").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "historical-conversation".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    let first = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id.clone(),
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: message("First"),
                run: run_spec(&revision_id),
                idempotency_key: "historical-first".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    complete_with_reply(&store, &first.run.id, &first.turn.user_commit_id, NOW + 2).await;
    store
        .maintain_candidate_projections(NOW + 3, "historical-projector", 10)
        .await
        .unwrap();
    let first_head = store
        .get_conversation_view(&conversation.id)
        .await
        .unwrap()
        .active_head_commit_id;
    let second = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id.clone(),
                expected_head_commit_id: first_head.clone(),
                user_content: message("Later"),
                run: run_spec(&revision_id),
                idempotency_key: "historical-second".into(),
            },
            NOW + 4,
        )
        .await
        .unwrap();
    let command = SelectConversationCandidateCommand {
        turn_id: first.turn.id,
        selected_run_id: first.run.id,
        expected_conversation_head_commit_id: second.turn.user_commit_id.clone(),
        idempotency_key: "historical-select".into(),
    };
    let selection = store
        .select_conversation_candidate_at(command.clone(), NOW + 5)
        .await
        .unwrap();
    assert_eq!(
        store
            .select_conversation_candidate_at(command, NOW + 6)
            .await
            .unwrap(),
        selection
    );
    assert_eq!(selection.selected_commit_id, first_head);
    assert_ne!(selection.selected_branch_id, first.candidate.branch_id);
    let original = store
        .get_working_context(&conversation.context_id, &first.candidate.branch_id)
        .await
        .unwrap();
    assert_eq!(original.head_commit_id, second.turn.user_commit_id);
    let historical = store
        .get_working_context(&conversation.context_id, &selection.selected_branch_id)
        .await
        .unwrap();
    assert_eq!(historical.value["messages"].as_array().unwrap().len(), 2);
}

fn message(text: &str) -> Vec<LlmContentPartIr> {
    vec![LlmContentPartIr::Text { text: text.into() }]
}
