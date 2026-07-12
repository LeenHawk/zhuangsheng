use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_core::{
    application::tool::PublishToolCommand,
    graph::{EffectClassification, ToolEffectSpec},
    llm::{ToolDescriptor, ToolLimits},
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn channel_and_preset_http_flow_publish_immutable_configuration() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
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
    let preview = call(
        &app,
        request(
            "POST",
            &format!("/v1/context-presets/{preset_id}/preview"),
            json!({
                "versionId":version["id"],
                "nodeInput":{},
                "sampleBindings":{},
                "budget":{
                    "contextWindowTokens":16384,
                    "reservedOutputTokens":2048,
                    "fixedRequestTokens":0,
                    "safetyMarginTokens":512,
                    "countSource":"estimate"
                }
            }),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(preview["contentMode"], "metadata_only");
    assert_eq!(preview["countSource"], "estimate");
    assert_eq!(preview["items"][0]["itemId"], "character");
    assert!(preview["items"][0]["tokenCount"].as_u64().unwrap() > 0);
    assert!(
        !serde_json::to_string(&preview)
            .unwrap()
            .contains("You are Alice")
    );
    let unsupported_count = call(
        &app,
        request(
            "POST",
            &format!("/v1/context-presets/{preset_id}/preview"),
            json!({
                "nodeInput":{},"sampleBindings":{},
                "budget":{"contextWindowTokens":16384,"reservedOutputTokens":2048,
                    "fixedRequestTokens":0,"safetyMarginTokens":512,"countSource":"provider"}
            }),
            &[],
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(unsupported_count["error"]["code"], "invalid_argument");
}

#[tokio::test]
async fn tool_descriptor_http_listing_excludes_executor_metadata() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: ToolDescriptor {
                tool_id: "web-search".into(),
                version: "1".into(),
                name: "web_search".into(),
                description: Some("Search public sources".into()),
                input_schema: JsonSchemaSpec {
                    schema_version: 1,
                    dialect: DIALECT_2020_12.into(),
                    validation_profile_version: 1,
                    format_policy_version: 1,
                    document: json!({
                        "type":"object",
                        "required":["query"],
                        "additionalProperties":false,
                        "properties":{"query":{"type":"string"}}
                    }),
                    limits: JsonSchemaLimits::default(),
                },
                binding_config_schema: None,
                effect: ToolEffectSpec {
                    classification: EffectClassification::Idempotent,
                    operation_key: "tool.web_search".into(),
                    requires_approval: false,
                },
                supports_parallel: true,
                required_scopes: Vec::new(),
                limits: ToolLimits {
                    timeout_ms: 5_000,
                    max_input_bytes: 4096,
                    max_llm_result_bytes: 16_384,
                    max_artifact_bytes: 1024 * 1024,
                },
            },
            implementation_digest: "sha256:web-search-implementation".into(),
            executor_key: "builtin.web-search".into(),
            enabled: true,
            idempotency_key: "publish-web-search".into(),
        })
        .await
        .unwrap();
    let app = test_app(store);
    let descriptors = call(
        &app,
        request("GET", "/v1/tools/descriptors", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(descriptors[0]["toolId"], "web-search");
    let encoded = serde_json::to_string(&descriptors).unwrap();
    assert!(!encoded.contains("executorKey"));
    assert!(!encoded.contains("implementationDigest"));
    assert!(!encoded.contains("builtin.web-search"));
}
