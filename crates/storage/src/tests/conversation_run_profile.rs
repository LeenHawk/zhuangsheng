use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::{
        conversation::{CreateConversationCommand, UpdateConversationRunProfileCommand},
        graph::{ApplyGraphCommand, UpdateGraphDraftCommand},
    },
    conversation::{
        ConversationInputShape, ConversationRunSpec, assistant_reply_payload_v1_schema,
        conversation_run_input_v1_schema,
    },
};

use crate::{
    SqliteStore, StorageError,
    graph::helpers::sql,
    tests::{applied_graph, graph, store, valid_draft},
};

const NOW: i64 = 1_700_000_100_000;

#[tokio::test]
async fn compatible_default_and_updated_run_profiles_are_versioned_and_idempotent() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "profile").await;
    let run = run_spec(&revision_id);
    let create_with_default = CreateConversationCommand {
        title: Some("Default story".into()),
        default_run: Some(run.clone()),
        idempotency_key: "conversation-with-default".into(),
    };
    let with_default = store
        .create_conversation_at(create_with_default.clone(), NOW)
        .await
        .unwrap();
    let default_profile = with_default.run_profile.clone().unwrap();
    assert_eq!(default_profile.run, run);
    assert_eq!(default_profile.revision_no, 1);
    assert_eq!(
        store
            .create_conversation_at(create_with_default, NOW + 1)
            .await
            .unwrap(),
        with_default
    );

    let without_default = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "conversation-without-default".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    let command = UpdateConversationRunProfileCommand {
        conversation_id: without_default.id.clone(),
        expected_revision_no: 0,
        run: run.clone(),
        idempotency_key: "profile-update-one".into(),
    };
    let first = store
        .update_conversation_run_profile_at(command.clone(), NOW + 2)
        .await
        .unwrap();
    assert_eq!(first.revision_no, 1);
    assert_eq!(
        store
            .update_conversation_run_profile_at(command, NOW + 3)
            .await
            .unwrap(),
        first
    );
    assert!(matches!(
        store
            .update_conversation_run_profile_at(
                UpdateConversationRunProfileCommand {
                    conversation_id: without_default.id.clone(),
                    expected_revision_no: 1,
                    run: run.clone(),
                    idempotency_key: "profile-update-one".into(),
                },
                NOW + 3,
            )
            .await,
        Err(StorageError::IdempotencyConflict)
    ));
    assert_eq!(
        store
            .get_conversation_view(&without_default.id)
            .await
            .unwrap()
            .run_profile,
        Some(first.clone())
    );
    let second = store
        .update_conversation_run_profile_at(
            UpdateConversationRunProfileCommand {
                conversation_id: without_default.id.clone(),
                expected_revision_no: 1,
                run,
                idempotency_key: "profile-update-two".into(),
            },
            NOW + 4,
        )
        .await
        .unwrap();
    assert_eq!(second.revision_no, 2);
    assert!(matches!(
        store
            .update_conversation_run_profile_at(
                UpdateConversationRunProfileCommand {
                    conversation_id: without_default.id,
                    expected_revision_no: 1,
                    run: second.run,
                    idempotency_key: "profile-update-stale".into(),
                },
                NOW + 5,
            )
            .await,
        Err(StorageError::Conflict(
            "conversation_run_profile_revision_conflict"
        ))
    ));
}

#[tokio::test]
async fn incompatible_graph_contract_leaves_no_conversation_or_profile_write() {
    let store = store().await;
    let incompatible = applied_graph(&store, "profile-incompatible").await;
    let result = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: Some(run_spec(&incompatible.id)),
                idempotency_key: "invalid-default-profile".into(),
            },
            NOW,
        )
        .await;
    assert!(matches!(result, Err(StorageError::InvalidArgument(_))));
    let count: i64 = store
        .db
        .query_one_raw(sql("SELECT COUNT(*) AS count FROM conversations", vec![]))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(count, 0);
}

pub(super) async fn compatible_revision(store: &SqliteStore, key: &str) -> String {
    let graph = graph(store, &format!("create-{key}")).await;
    let current = store.get_graph_draft(&graph.id).await.unwrap();
    let mut draft = valid_draft(&graph.id, "Conversation Graph");
    draft.run_input_schema = Some(conversation_run_input_v1_schema());
    draft.output_contract[0].schema = Some(assistant_reply_payload_v1_schema());
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: current.revision_token,
            document: draft,
            idempotency_key: format!("draft-{key}"),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap()
        .id
}

pub(super) fn run_spec(revision_id: &str) -> ConversationRunSpec {
    ConversationRunSpec {
        graph_revision_id: revision_id.into(),
        reply_output_key: "reply".into(),
        input_shape: ConversationInputShape::ConversationMessageV1,
    }
}
