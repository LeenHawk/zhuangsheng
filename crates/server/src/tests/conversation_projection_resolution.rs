use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn candidate_projection_resolution_http_decodes_the_public_command() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let response = call(
        &app,
        request(
            "POST",
            "/v1/turns/turn_missing/candidates/run_missing/projection-resolution",
            json!({
                "expectedCurrentBranchHead":"commit_missing",
                "resolution":{
                    "type":"append_after_current",
                    "reason":"operator reviewed the intervening diff"
                }
            }),
            &[("idempotency-key", "resolve-projection-http".into())],
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(response["error"]["code"], "not_found");
}
