use zhuangsheng_core::application::graph::{CreateGraphCommand, UpdateGraphDraftCommand};

use crate::StorageError;

use super::{graph, store, valid_draft};

#[tokio::test]
async fn create_is_idempotent_and_initializes_empty_draft() {
    let store = store().await;
    let first = graph(&store, "create-1").await;
    let replay = store
        .create_graph(CreateGraphCommand {
            name: "Story Graph".into(),
            idempotency_key: "create-1".into(),
        })
        .await
        .unwrap();

    assert_eq!(first.id, replay.graph.id);
    assert_eq!(
        replay.draft_revision_token,
        store
            .get_graph_draft(&first.id)
            .await
            .unwrap()
            .revision_token
    );
    assert_eq!(store.list_graphs().await.unwrap().len(), 1);
    let draft = store.get_graph_draft(&first.id).await.unwrap();
    assert!(draft.document.nodes.is_empty());
    assert!(draft.document.edges.is_empty());
    assert!(draft.document.output_contract.is_empty());

    let conflict = store
        .create_graph(CreateGraphCommand {
            name: "Different".into(),
            idempotency_key: "create-1".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(conflict, StorageError::IdempotencyConflict));
}

#[tokio::test]
async fn draft_uses_cas_and_replays_historical_result() {
    let store = store().await;
    let graph = graph(&store, "create-2").await;
    let initial = store.get_graph_draft(&graph.id).await.unwrap();
    let first_command = UpdateGraphDraftCommand {
        graph_id: graph.id.clone(),
        expected_revision_token: initial.revision_token.clone(),
        document: valid_draft(&graph.id, "First"),
        idempotency_key: "draft-1".into(),
    };
    let first = store
        .update_graph_draft(first_command.clone())
        .await
        .unwrap();
    let second = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: first.revision_token.clone(),
            document: valid_draft(&graph.id, "Second"),
            idempotency_key: "draft-2".into(),
        })
        .await
        .unwrap();

    let replay = store.update_graph_draft(first_command).await.unwrap();
    assert_eq!(replay.revision_token, first.revision_token);
    assert_eq!(replay.document.name.as_deref(), Some("First"));
    assert_eq!(
        store
            .get_graph_draft(&graph.id)
            .await
            .unwrap()
            .revision_token,
        second.revision_token
    );
    let create_replay = store
        .create_graph(CreateGraphCommand {
            name: "Story Graph".into(),
            idempotency_key: "create-2".into(),
        })
        .await
        .unwrap();
    assert_eq!(create_replay.draft_revision_token, initial.revision_token);

    let stale = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: initial.revision_token,
            document: valid_draft(&graph.id, "Stale"),
            idempotency_key: "draft-stale".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        stale,
        StorageError::Conflict("graph_draft_revision")
    ));
}
