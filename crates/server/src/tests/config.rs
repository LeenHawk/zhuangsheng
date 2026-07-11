use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{app, call, request};

#[tokio::test]
async fn channel_and_preset_http_flow_publish_immutable_configuration() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = app(
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store.clone(),
        store,
    );
    let channel = call(
        &app,
        request(
            "POST",
            "/v1/channels",
            json!({"name":"Local Compatible API"}),
            &[("idempotency-key", "channel-http-create".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let channel_id = channel["id"].as_str().unwrap();
    assert!(channel["headRevisionId"].is_null());
    let channel_revision = call(
        &app,
        request(
            "POST",
            &format!("/v1/channels/{channel_id}/revisions"),
            json!({
                "expectedHeadRevisionId":null,
                "spec":{
                    "operationTaxonomyVersion":1,
                    "adapterDecoderVersion":1,
                    "baseUrl":"https://llm.example.test/v1/",
                    "transportPolicy":{
                        "allowLoopbackHttp":false,
                        "allowUnauthenticated":true
                    },
                    "credential":{"type":"none"},
                    "operationKeys":[{
                        "operation":"generate_content",
                        "kind":"open_ai_responses"
                    }],
                    "modelCatalogs":[{
                        "operationKey":{
                            "operation":"generate_content",
                            "kind":"open_ai_responses"
                        },
                        "policy":"open",
                        "models":[]
                    }],
                    "capabilities":[]
                }
            }),
            &[("idempotency-key", "channel-http-publish".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(channel_revision["revisionNo"], 1);
    assert_eq!(channel_revision["baseUrl"], "https://llm.example.test/v1");

    let preset = call(
        &app,
        request(
            "POST",
            "/v1/context-presets",
            json!({"name":"Character Preset"}),
            &[("idempotency-key", "preset-http-create".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let preset_id = preset["id"].as_str().unwrap();
    let version = call(
        &app,
        request(
            "POST",
            &format!("/v1/context-presets/{preset_id}/revisions"),
            json!({
                "expectedHeadVersionId":null,
                "spec":{
                    "mode":"chat",
                    "items":[{
                        "id":"character",
                        "enabled":true,
                        "requestedRole":"system",
                        "source":{"type":"literal","text":"You are Alice."},
                        "position":{"type":"start"},
                        "budget":{"required":true}
                    }]
                }
            }),
            &[("idempotency-key", "preset-http-publish".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(version["versionNo"], 1);
    assert_eq!(version["spec"]["preview"]["content"], "metadata_only");
}
