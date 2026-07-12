use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::conversation::{
        CreateConversationCommand, SelectConversationCandidateCommand,
        SubmitConversationTurnCommand,
    },
    llm::ir::LlmContentPartIr,
};

use crate::{StorageError, graph::helpers::sql, tests::store};

use super::conversation_projection_support::complete_with_reply;
use super::conversation_run_profile::{compatible_revision, run_spec};

const NOW: i64 = 1_700_000_300_000;

#[tokio::test]
async fn terminal_reconciliation_projects_one_assistant_message_idempotently() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "projection").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "projection-conversation".into(),
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
                    text: "Tell me what waits inside".into(),
                }],
                run: run_spec(&revision_id),
                idempotency_key: "projection-turn".into(),
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
    assert_eq!(
        store
            .maintain_candidate_projections(NOW + 3, "projector-test", 10)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .maintain_candidate_projections(NOW + 4, "projector-test", 10)
            .await
            .unwrap(),
        0
    );
    let row = store.db.query_one_raw(sql(
        "SELECT status, assistant_message_id, candidate_commit_id FROM turn_candidates WHERE run_id = ?",
        vec![submitted.run.id.clone().into()],
    )).await.unwrap().unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "ready");
    assert!(
        row.try_get::<Option<String>>("", "assistant_message_id")
            .unwrap()
            .is_some()
    );
    let candidate_commit_id: String = row.try_get("", "candidate_commit_id").unwrap();
    let selected_conversation = store.get_conversation_view(&conversation.id).await.unwrap();
    assert_eq!(
        selected_conversation.active_branch_id,
        submitted.candidate.branch_id
    );
    assert_eq!(
        selected_conversation.active_head_commit_id,
        candidate_commit_id
    );
    let explicit = SelectConversationCandidateCommand {
        turn_id: submitted.turn.id.clone(),
        selected_run_id: submitted.run.id.clone(),
        expected_conversation_head_commit_id: selected_conversation.active_head_commit_id,
        idempotency_key: "projection-explicit-select".into(),
    };
    let selection = store
        .select_conversation_candidate_at(explicit.clone(), NOW + 5)
        .await
        .unwrap();
    assert_eq!(selection.selected_run_id, submitted.run.id);
    assert_eq!(
        store
            .select_conversation_candidate_at(explicit, NOW + 6)
            .await
            .unwrap(),
        selection
    );
    assert!(matches!(
        store
            .select_conversation_candidate_at(
                SelectConversationCandidateCommand {
                    turn_id: submitted.turn.id.clone(),
                    selected_run_id: submitted.run.id.clone(),
                    expected_conversation_head_commit_id: submitted.turn.user_commit_id.clone(),
                    idempotency_key: "projection-explicit-stale".into(),
                },
                NOW + 7,
            )
            .await,
        Err(StorageError::Conflict("conversation_head"))
    ));
    let candidate_context = store
        .get_working_context(&conversation.context_id, &submitted.candidate.branch_id)
        .await
        .unwrap();
    let messages = candidate_context.value["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["originRunId"], submitted.run.id);
    let active_context = store
        .get_working_context(&conversation.context_id, &conversation.active_branch_id)
        .await
        .unwrap();
    assert_eq!(
        active_context.value["messages"].as_array().unwrap().len(),
        1
    );
}
