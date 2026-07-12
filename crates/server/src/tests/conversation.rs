use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn conversation_http_bootstraps_and_reloads_a_durable_root() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store.clone());
    let body = json!({"title":"The Moonlit Archive"});
    let created = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            body.clone(),
            &[("idempotency-key", "conversation-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(created["title"], body["title"]);
    assert!(created["id"].as_str().unwrap().starts_with("conversation_"));
    let conversation_id = created["id"].as_str().unwrap();
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
    assert_eq!(loaded, created);
    let listed = call(
        &app,
        request("GET", "/v1/conversations", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(listed["items"][0], created);
    let timeline = call(
        &app,
        request(
            "GET",
            &format!("/v1/conversations/{conversation_id}/turns"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(timeline["messages"], json!([]));
    assert_eq!(timeline["turns"], json!([]));
    let replayed = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            body,
            &[("idempotency-key", "conversation-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(replayed, created);
    let context = store
        .get_working_context(
            created["contextId"].as_str().unwrap(),
            created["activeBranchId"].as_str().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(context.value, json!({"schemaVersion":1,"messages":[]}));
}

#[tokio::test]
async fn conversation_http_enforces_idempotency_and_not_found_contracts() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let missing_key = call(
        &app,
        request("POST", "/v1/conversations", json!({}), &[]),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(missing_key["error"]["code"], "missing_header");
    call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({"title":"First"}),
            &[("idempotency-key", "conversation-conflict".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let conflict = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({"title":"Second"}),
            &[("idempotency-key", "conversation-conflict".into())],
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");
    let missing = call(
        &app,
        request(
            "GET",
            "/v1/conversations/conversation_missing",
            json!(null),
            &[],
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(missing["error"]["code"], "not_found");
}
