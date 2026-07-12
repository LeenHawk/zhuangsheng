use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::conversation::{CreateConversationCommand, SubmitConversationTurnCommand},
    llm::ir::LlmContentPartIr,
};

use crate::{graph::helpers::sql, tests::store};

use super::{
    conversation_projection_support::terminalize_failed,
    conversation_run_profile::{compatible_revision, run_spec},
};

const NOW: i64 = 1_700_000_350_000;

#[tokio::test]
async fn terminal_reconciliation_marks_failed_candidate_without_message() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "projection-failed").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "projection-failed-conversation".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    let submitted = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id,
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: vec![LlmContentPartIr::Text {
                    text: "fail".into(),
                }],
                run: run_spec(&revision_id),
                idempotency_key: "projection-failed-turn".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    terminalize_failed(&store, &submitted.run.id, NOW + 2).await;
    assert_eq!(
        store
            .maintain_candidate_projections(NOW + 3, "projector-test", 10)
            .await
            .unwrap(),
        1
    );
    let status = store
        .db
        .query_one_raw(sql(
            "SELECT status FROM turn_candidates WHERE run_id = ?",
            vec![submitted.run.id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "status")
        .unwrap();
    assert_eq!(status, "failed");
    let assistant_count = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM conversation_messages WHERE role = 'assistant'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(assistant_count, 0);
}
