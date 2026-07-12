use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::conversation::{
        CreateConversationCommand, RegenerateConversationCandidateCommand,
        SelectConversationCandidateCommand, SubmitConversationTurnCommand,
    },
    llm::ir::LlmContentPartIr,
};

use crate::{StorageError, graph::helpers::sql, tests::store};

use super::{
    conversation_projection_support::complete_with_reply,
    conversation_run_profile::{compatible_revision, run_spec},
};

const NOW: i64 = 1_700_000_500_000;

#[tokio::test]
async fn regenerate_reuses_user_commit_and_swipe_selects_the_second_ready_candidate() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "regenerate").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "regenerate-conversation".into(),
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
                user_content: vec![LlmContentPartIr::Text {
                    text: "Show another path".into(),
                }],
                run: run_spec(&revision_id),
                idempotency_key: "regenerate-turn".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    complete_with_reply(&store, &first.run.id, &first.turn.user_commit_id, NOW + 2).await;
    store
        .maintain_candidate_projections(NOW + 3, "regenerate-projector", 10)
        .await
        .unwrap();
    let first_selection = store.get_conversation_view(&conversation.id).await.unwrap();
    let command = RegenerateConversationCandidateCommand {
        turn_id: first.turn.id.clone(),
        expected_user_commit_id: first.turn.user_commit_id.clone(),
        run: run_spec(&revision_id),
        idempotency_key: "regenerate-second".into(),
    };
    let second = store
        .regenerate_conversation_candidate_at(command.clone(), NOW + 4)
        .await
        .unwrap();
    assert_ne!(second.run.id, first.run.id);
    assert_eq!(second.run.input_commit_id, first.turn.user_commit_id);
    assert_eq!(second.candidate.base_commit_id, first.turn.user_commit_id);
    assert_eq!(
        store
            .regenerate_conversation_candidate_at(command.clone(), NOW + 5)
            .await
            .unwrap(),
        second
    );
    let mut conflicting_replay = command.clone();
    conflicting_replay.run.reply_output_key = "missing".into();
    assert!(matches!(
        store
            .regenerate_conversation_candidate_at(conflicting_replay, NOW + 5)
            .await,
        Err(StorageError::IdempotencyConflict)
    ));
    let mut stale = command;
    stale.expected_user_commit_id = "commit_stale".into();
    stale.idempotency_key = "regenerate-stale".into();
    assert!(matches!(
        store
            .regenerate_conversation_candidate_at(stale, NOW + 5)
            .await,
        Err(StorageError::Conflict("conversation_user_commit"))
    ));
    assert_eq!(
        store
            .get_conversation_view(&conversation.id)
            .await
            .unwrap()
            .active_head_commit_id,
        first_selection.active_head_commit_id
    );
    complete_with_reply(&store, &second.run.id, &first.turn.user_commit_id, NOW + 6).await;
    store
        .maintain_candidate_projections(NOW + 7, "regenerate-projector", 10)
        .await
        .unwrap();
    let still_first = store.get_conversation_view(&conversation.id).await.unwrap();
    assert_eq!(
        still_first.active_head_commit_id,
        first_selection.active_head_commit_id
    );
    let selected = store
        .select_conversation_candidate_at(
            SelectConversationCandidateCommand {
                turn_id: first.turn.id,
                selected_run_id: second.run.id.clone(),
                expected_conversation_head_commit_id: still_first.active_head_commit_id,
                idempotency_key: "swipe-second".into(),
            },
            NOW + 8,
        )
        .await
        .unwrap();
    assert_eq!(selected.selected_run_id, second.run.id);
    assert_eq!(selected.selected_branch_id, second.candidate.branch_id);
    let user_count = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM conversation_messages WHERE role = 'user'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(user_count, 1);
}
