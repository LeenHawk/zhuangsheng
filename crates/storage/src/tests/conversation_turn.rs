use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::conversation::{CreateConversationCommand, SubmitConversationTurnCommand},
    llm::ir::LlmContentPartIr,
};

use crate::{StorageError, graph::helpers::sql, tests::store};

use super::conversation_run_profile::{compatible_revision, run_spec};

const NOW: i64 = 1_700_000_200_000;

#[tokio::test]
async fn submit_turn_atomically_appends_user_message_and_starts_candidate_run() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "turn").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "turn-conversation".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    let command = SubmitConversationTurnCommand {
        conversation_id: conversation.id.clone(),
        expected_head_commit_id: conversation.active_head_commit_id.clone(),
        user_content: vec![LlmContentPartIr::Text {
            text: "Enter the moonlit archive".into(),
        }],
        run: run_spec(&revision_id),
        idempotency_key: "submit-turn-one".into(),
    };
    let submitted = store
        .submit_conversation_turn_at(command.clone(), NOW + 1)
        .await
        .unwrap();
    assert_eq!(submitted.run.id, submitted.candidate.run_id);
    assert_eq!(submitted.run.branch_id, submitted.candidate.branch_id);
    assert_eq!(submitted.run.input_commit_id, submitted.turn.user_commit_id);
    let active = store.get_conversation_view(&conversation.id).await.unwrap();
    assert_eq!(active.active_head_commit_id, submitted.turn.user_commit_id);
    assert_eq!(active.active_branch_id, conversation.active_branch_id);
    let context = store
        .get_working_context(&active.context_id, &active.active_branch_id)
        .await
        .unwrap();
    assert_eq!(context.value["messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        context.value["messages"][0]["messageId"],
        submitted.turn.user_message_id
    );
    assert_eq!(
        store
            .submit_conversation_turn_at(command, NOW + 2)
            .await
            .unwrap(),
        submitted
    );
    for (table, expected) in [
        ("conversation_messages", 1),
        ("conversation_turns", 1),
        ("turn_candidates", 1),
        ("conversation_run_bindings", 1),
        ("graph_runs", 1),
    ] {
        assert_eq!(count(&store, table).await, expected, "{table}");
    }
    assert!(matches!(
        store
            .submit_conversation_turn_at(
                SubmitConversationTurnCommand {
                    conversation_id: conversation.id,
                    expected_head_commit_id: conversation.active_head_commit_id,
                    user_content: vec![LlmContentPartIr::Text {
                        text: "stale".into()
                    }],
                    run: run_spec(&revision_id),
                    idempotency_key: "submit-turn-stale".into(),
                },
                NOW + 3,
            )
            .await,
        Err(StorageError::Conflict("conversation_head"))
    ));
    assert_eq!(count(&store, "conversation_messages").await, 1);
}

#[tokio::test]
async fn invalid_turn_content_or_contract_rolls_back_every_domain_and_runtime_row() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "invalid-turn").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "invalid-turn-conversation".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    let mut command = SubmitConversationTurnCommand {
        conversation_id: conversation.id.clone(),
        expected_head_commit_id: conversation.active_head_commit_id.clone(),
        user_content: vec![],
        run: run_spec(&revision_id),
        idempotency_key: "invalid-turn-empty".into(),
    };
    assert!(matches!(
        store
            .submit_conversation_turn_at(command.clone(), NOW + 1)
            .await,
        Err(StorageError::InvalidArgument(_))
    ));
    command.user_content = vec![LlmContentPartIr::Text {
        text: "valid".into(),
    }];
    command.run.reply_output_key = "missing".into();
    command.idempotency_key = "invalid-turn-contract".into();
    assert!(matches!(
        store.submit_conversation_turn_at(command, NOW + 2).await,
        Err(StorageError::InvalidArgument(_))
    ));
    for table in [
        "conversation_messages",
        "conversation_turns",
        "turn_candidates",
        "conversation_run_bindings",
        "graph_runs",
    ] {
        assert_eq!(count(&store, table).await, 0, "{table}");
    }
    assert_eq!(
        store
            .get_conversation_view(&conversation.id)
            .await
            .unwrap()
            .active_head_commit_id,
        conversation.active_head_commit_id
    );
}

async fn count(store: &crate::SqliteStore, table: &str) -> i64 {
    let allowed = [
        "conversation_messages",
        "conversation_turns",
        "turn_candidates",
        "conversation_run_bindings",
        "graph_runs",
    ];
    assert!(allowed.contains(&table));
    store
        .db
        .query_one_raw(sql(
            &format!("SELECT COUNT(*) AS count FROM {table}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
