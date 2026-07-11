use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    DomainError,
    application::graph::{ApplyGraphCommand, UpdateGraphDraftCommand},
};

use crate::{StorageError, graph::helpers::sql};

use super::{graph, store, valid_draft};

#[tokio::test]
async fn valid_apply_persists_immutable_revision_and_reuses_content() {
    let store = store().await;
    let graph = graph(&store, "create-apply").await;
    let initial = store.get_graph_draft(&graph.id).await.unwrap();
    let draft = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: initial.revision_token,
            document: valid_draft(&graph.id, "Applied"),
            idempotency_key: "draft-apply".into(),
        })
        .await
        .unwrap();
    let command = |key: &str| ApplyGraphCommand {
        graph_id: graph.id.clone(),
        expected_revision_token: draft.revision_token.clone(),
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        idempotency_key: key.into(),
    };

    let first = store.apply_graph(command("apply-1")).await.unwrap();
    let replay = store.apply_graph(command("apply-1")).await.unwrap();
    let same_content = store.apply_graph(command("apply-2")).await.unwrap();
    let loaded = store.get_graph_revision(&first.id).await.unwrap();

    assert_eq!(first.id, replay.id);
    assert_eq!(first.id, same_content.id);
    assert_eq!(first.created_at, same_content.created_at);
    assert_eq!(loaded.content_hash, first.content_hash);
    assert_eq!(loaded.definition, first.definition);

    let count: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM graph_revisions WHERE graph_id = ?",
            vec![graph.id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(count, 1);

    let violations = store
        .db
        .query_all(sql("PRAGMA foreign_key_check", vec![]))
        .await
        .unwrap();
    assert!(violations.is_empty());
}

#[tokio::test]
async fn invalid_graph_and_unknown_versions_leave_no_revision() {
    let store = store().await;
    let graph = graph(&store, "create-invalid").await;
    let draft = store.get_graph_draft(&graph.id).await.unwrap();
    let apply = |key: &str, taxonomy| ApplyGraphCommand {
        graph_id: graph.id.clone(),
        expected_revision_token: draft.revision_token.clone(),
        operation_taxonomy_version: taxonomy,
        adapter_decoder_version: 1,
        idempotency_key: key.into(),
    };

    let invalid = store
        .apply_graph(apply("invalid-graph", 1))
        .await
        .unwrap_err();
    assert!(matches!(
        invalid,
        StorageError::Domain(DomainError::GraphValidation(_))
    ));

    let unknown = store
        .apply_graph(apply("unknown-version", 99))
        .await
        .unwrap_err();
    assert!(matches!(
        unknown,
        StorageError::Domain(DomainError::GraphValidation(ref issues))
            if issues.iter().any(|issue| issue.code == "unsupported_operation_taxonomy")
    ));

    let count: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM graph_revisions WHERE graph_id = ?",
            vec![graph.id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(count, 0);
}
