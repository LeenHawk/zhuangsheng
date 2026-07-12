use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::application::conversation::CreateConversationCommand;

use crate::{StorageError, graph::helpers::sql, tests::store};

const NOW: i64 = 1_700_000_000_000;

#[tokio::test]
async fn conversation_root_is_atomic_durable_and_idempotent() {
    let store = store().await;
    let command = CreateConversationCommand {
        title: Some("The Moonlit Archive".into()),
        idempotency_key: "create-conversation-root".into(),
    };
    let created = store
        .create_conversation_at(command.clone(), NOW)
        .await
        .unwrap();
    assert_eq!(created.title, command.title);
    assert_eq!(
        store.get_conversation_view(&created.id).await.unwrap(),
        created
    );
    let context = store
        .get_working_context(&created.context_id, &created.active_branch_id)
        .await
        .unwrap();
    assert_eq!(context.head_commit_id, created.active_head_commit_id);
    assert_eq!(context.value, json!({"schemaVersion":1,"messages":[]}));
    let commit = store
        .get_context_commit(&created.active_head_commit_id)
        .await
        .unwrap();
    assert_eq!(commit.sequence_no, 1);
    assert!(commit.parent_commit_ids.is_empty());
    assert_eq!(
        commit.operation_id,
        format!("conversation-root:{}", created.id)
    );
    assert_eq!(
        store
            .create_conversation_at(command, NOW + 1)
            .await
            .unwrap(),
        created
    );
    for table in [
        "conversations",
        "contexts",
        "context_branches",
        "version_commits",
        "materialized_projections",
        "domain_events",
    ] {
        assert_eq!(count(&store, table).await, 1, "table {table}");
    }
    assert_eq!(count(&store, "application_command_receipts").await, 1);
    assert_eq!(count(&store, "content_object_refs").await, 2);

    assert!(matches!(
        store
            .create_conversation_at(
                CreateConversationCommand {
                    title: Some("Different title".into()),
                    idempotency_key: "create-conversation-root".into(),
                },
                NOW + 2,
            )
            .await,
        Err(StorageError::IdempotencyConflict)
    ));
}

#[tokio::test]
async fn invalid_or_corrupt_conversation_root_fails_closed() {
    let store = store().await;
    assert!(matches!(
        store
            .create_conversation_at(
                CreateConversationCommand {
                    title: Some("bad\ntitle".into()),
                    idempotency_key: "invalid-conversation".into(),
                },
                NOW,
            )
            .await,
        Err(StorageError::InvalidArgument(_))
    ));
    assert_eq!(count(&store, "conversations").await, 0);
    let created = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                idempotency_key: "valid-conversation".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    store
        .db
        .execute_raw(sql(
            "UPDATE materialized_projections SET projection_json = '{\"schemaVersion\":1,\"messages\":{}}' WHERE aggregate_kind = 'working_context' AND aggregate_id = ?",
            vec![created.context_id.into()],
        ))
        .await
        .unwrap();
    assert!(matches!(
        store.get_conversation_view(&created.id).await,
        Err(StorageError::Integrity(_))
    ));
}

async fn count(store: &crate::SqliteStore, table: &str) -> i64 {
    let query = match table {
        "conversations" => "SELECT COUNT(*) AS count FROM conversations",
        "contexts" => "SELECT COUNT(*) AS count FROM contexts",
        "context_branches" => "SELECT COUNT(*) AS count FROM context_branches",
        "version_commits" => "SELECT COUNT(*) AS count FROM version_commits",
        "materialized_projections" => "SELECT COUNT(*) AS count FROM materialized_projections",
        "domain_events" => "SELECT COUNT(*) AS count FROM domain_events",
        "application_command_receipts" => {
            "SELECT COUNT(*) AS count FROM application_command_receipts"
        }
        "content_object_refs" => "SELECT COUNT(*) AS count FROM content_object_refs",
        _ => unreachable!(),
    };
    store
        .db
        .query_one_raw(sql(query, vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
