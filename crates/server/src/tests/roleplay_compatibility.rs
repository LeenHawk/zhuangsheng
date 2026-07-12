use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, conversation_profile::compatible_revision, request, test_app};

#[tokio::test]
async fn roleplay_compatibility_http_exposes_server_projection() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = compatible_revision(&store).await;
    let app = test_app(store);

    let options = call(
        &app,
        request("GET", "/v1/roleplay/graph-options", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    let option = options
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["revisionId"] == revision_id)
        .unwrap();
    assert_eq!(option["graphName"], "Conversation HTTP Graph");
    assert_eq!(option["replyOutputKeys"], json!(["reply"]));
    assert_eq!(option["compatibility"]["mode"], "expert_only");

    let compatibility = call(
        &app,
        request(
            "GET",
            &format!("/v1/graph-revisions/{revision_id}/roleplay-compatibility"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(compatibility["mode"], "expert_only");
    assert_eq!(
        compatibility["reasons"],
        json!(["primary_llm_node_not_unique"])
    );
}
