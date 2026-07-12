use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_core::{
    application::graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
    conversation::{assistant_reply_payload_v1_schema, conversation_run_input_v1_schema},
    graph::GraphDraft,
};
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn conversation_profile_http_uses_exact_graph_contract_and_revision_cas() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = compatible_revision(&store).await;
    let app = test_app(store);
    let run = json!({
        "graphRevisionId":revision_id,
        "replyOutputKey":"reply",
        "inputShape":"conversation_message_v1"
    });
    let created = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({"title":"Profile HTTP","defaultRun":run}),
            &[("idempotency-key", "conversation-profile-create".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["runProfile"]["revisionNo"], 1);
    let conversation_id = created["id"].as_str().unwrap();
    let update_body = json!({"expectedRevisionNo":1,"run":run});
    let updated = call(
        &app,
        request(
            "PUT",
            &format!("/v1/conversations/{conversation_id}/run-profile"),
            update_body.clone(),
            &[("idempotency-key", "conversation-profile-update".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(updated["revisionNo"], 2);
    let replayed = call(
        &app,
        request(
            "PUT",
            &format!("/v1/conversations/{conversation_id}/run-profile"),
            update_body,
            &[("idempotency-key", "conversation-profile-update".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(replayed, updated);
    let stale = call(
        &app,
        request(
            "PUT",
            &format!("/v1/conversations/{conversation_id}/run-profile"),
            json!({"expectedRevisionNo":1,"run":run}),
            &[("idempotency-key", "conversation-profile-stale".into())],
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        stale["error"]["code"],
        "conversation_run_profile_revision_conflict"
    );
    let loaded = call(
        &app,
        request(
            "GET",
            &format!("/v1/conversations/{conversation_id}"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(loaded["runProfile"], updated);
}

async fn compatible_revision(store: &SqliteStore) -> String {
    let graph = store
        .create_graph(CreateGraphCommand {
            name: "Conversation HTTP Graph".into(),
            idempotency_key: "conversation-profile-graph".into(),
        })
        .await
        .unwrap();
    let current = store.get_graph_draft(&graph.graph.id).await.unwrap();
    let document: GraphDraft = serde_json::from_value(json!({
        "graphId":graph.graph.id,
        "name":"Conversation HTTP Graph",
        "nodes":[
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges":[{"from":{"nodeId":"input","output":"default"},"to":{"nodeId":"output","input":"default"}}],
        "runInputSchema":conversation_run_input_v1_schema(),
        "outputContract":[{"key":"reply","schema":assistant_reply_payload_v1_schema(),"collection":"single","required":true}],
        "limits":null
    }))
    .unwrap();
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.graph.id.clone(),
            expected_revision_token: current.revision_token,
            document,
            idempotency_key: "conversation-profile-graph-draft".into(),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: "conversation-profile-graph-apply".into(),
        })
        .await
        .unwrap()
        .id
}
