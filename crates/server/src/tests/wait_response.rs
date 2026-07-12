use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn wait_response_adapter_accepts_the_versioned_value_variant() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let response = call(
        &app,
        request(
            "POST",
            "/v1/waits/wait_missing/responses",
            json!({
                "deliveryId":"delivery_1",
                "response":{"type":"value","value":{"choice":"left"}}
            }),
            &[],
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(response["error"]["code"], "not_found");
}
