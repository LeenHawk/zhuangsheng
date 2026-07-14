use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn plugin_http_lists_installed_extensions() {
    let store = std::sync::Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let response = call(
        &app,
        request("GET", "/v1/plugins", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(response, json!([]));
}
