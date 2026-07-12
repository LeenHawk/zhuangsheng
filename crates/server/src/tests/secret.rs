use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::secret::SecretResolver,
    llm::{SecretRef, SecretScheme},
};
use zhuangsheng_storage::SqliteStore;

use super::{app, call, request};

const PASSWORD: &str = "correct horse battery staple";
const API_KEY: &str = "sk-http-secret-value";

#[tokio::test]
async fn secret_http_flow_never_returns_plaintext_and_expires_unlock_receipt() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = app(
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
    );
    let initial_status = call(
        &app,
        request("GET", "/v1/secret-store/status", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(initial_status["initialized"], false);
    let initialized = call(
        &app,
        request(
            "POST",
            "/v1/secret-store/initialize",
            json!({"masterPassword":PASSWORD}),
            &[("idempotency-key", "http-secret-init".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let session_id = initialized["sessionId"].as_str().unwrap();
    let metadata = call(
        &app,
        request(
            "PUT",
            "/v1/secrets/primary",
            json!({
                "name":"Primary",
                "kind":"api_key",
                "value":API_KEY,
                "sessionId":session_id
            }),
            &[("idempotency-key", "http-secret-put".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(metadata["secretRef"]["id"], "primary");
    assert!(!metadata.to_string().contains(API_KEY));
    let listed = call(
        &app,
        request("GET", "/v1/secrets", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert!(!listed.to_string().contains(API_KEY));
    let resolved = SecretResolver::resolve_secret(
        store.as_ref(),
        &SecretRef {
            scheme: SecretScheme::Secret,
            id: "primary".into(),
        },
    )
    .await
    .unwrap();
    resolved.with_bytes(|bytes| assert_eq!(bytes, API_KEY.as_bytes()));

    call(
        &app,
        request(
            "POST",
            "/v1/secret-store/lock",
            json!({"expectedSessionId":session_id}),
            &[("idempotency-key", "http-secret-lock".into())],
        ),
        StatusCode::OK,
    )
    .await;
    let wrong = call(
        &app,
        request(
            "POST",
            "/v1/secret-store/unlock",
            json!({"masterPassword":"wrong password long enough"}),
            &[("idempotency-key", "http-secret-unlock-wrong".into())],
        ),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    assert_eq!(wrong["error"]["code"], "secret_store_unlock_failed");
    let unlocked = call(
        &app,
        request(
            "POST",
            "/v1/secret-store/unlock",
            json!({"masterPassword":PASSWORD}),
            &[("idempotency-key", "http-secret-unlock".into())],
        ),
        StatusCode::OK,
    )
    .await;
    let unlocked_session = unlocked["sessionId"].as_str().unwrap();
    call(
        &app,
        request(
            "POST",
            "/v1/secret-store/lock",
            json!({"expectedSessionId":unlocked_session}),
            &[("idempotency-key", "http-secret-lock-2".into())],
        ),
        StatusCode::OK,
    )
    .await;
    let expired: Value = call(
        &app,
        request(
            "POST",
            "/v1/secret-store/unlock",
            json!({"masterPassword":PASSWORD}),
            &[("idempotency-key", "http-secret-unlock".into())],
        ),
        StatusCode::GONE,
    )
    .await;
    assert_eq!(expired["error"]["code"], "idempotency_key_expired");
}
