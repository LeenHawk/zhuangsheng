use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn effect_resolution_is_exposed_only_as_an_idempotent_runtime_command() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let body = json!({
        "expectedEffectAttemptId": "effectattempt_missing",
        "expectedRunControlEpoch": 0,
        "kind": "abort_run",
        "decision": {"reason": "operator chose isolation"},
        "resultObjectId": null,
        "evidenceObjectId": null
    });

    let missing_key = call(
        &app,
        request(
            "POST",
            "/v1/effects/effect_missing/resolution",
            body.clone(),
            &[],
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(missing_key["error"]["code"], "missing_header");

    let missing_effect = call(
        &app,
        request(
            "POST",
            "/v1/effects/effect_missing/resolution",
            body,
            &[("idempotency-key", "resolve-missing".into())],
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(missing_effect["error"]["code"], "effect_resolution_wait");
}
