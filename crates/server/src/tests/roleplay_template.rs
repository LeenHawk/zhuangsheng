use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn user_mode_template_builds_an_applied_compatible_graph_idempotently() {
    let app = test_app(Arc::new(
        SqliteStore::connect("sqlite::memory:").await.unwrap(),
    ));
    let channel = call(
        &app,
        request(
            "POST",
            "/v1/channels",
            json!({"name":"Role model"}),
            &[("idempotency-key", "template-channel".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let channel_id = channel["id"].as_str().unwrap();
    let first_channel_revision = call(
        &app,
        request(
            "POST",
            &format!("/v1/channels/{channel_id}/revisions"),
            json!({"expectedHeadRevisionId":null,"spec":{
                "operationTaxonomyVersion":1,"adapterDecoderVersion":1,
                "baseUrl":"https://llm.example.test/v1",
                "transportPolicy":{"allowLoopbackHttp":false,"allowUnauthenticated":true},
                "credential":{"type":"none"},
                "operationKeys":[{"operation":"generate_content","kind":"open_ai_responses"}],
                "modelCatalogs":[{"operationKey":{"operation":"generate_content","kind":"open_ai_responses"},"policy":"allowlist","models":[{"id":"role-model","capabilities":{"structuredOutput":true}}]}]
            }}),
            &[("idempotency-key", "template-channel-publish".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let preset = call(
        &app,
        request(
            "POST",
            "/v1/context-presets",
            json!({"name":"Character"}),
            &[("idempotency-key", "template-preset".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let preset_id = preset["id"].as_str().unwrap();
    call(
        &app,
        request(
            "POST",
            &format!("/v1/context-presets/{preset_id}/revisions"),
            json!({"expectedHeadVersionId":null,"spec":{"mode":"chat","items":[
                {
                    "id":"character","enabled":true,"requestedRole":"system",
                    "source":{"type":"literal","text":"You are Alice."},
                    "position":{"type":"start"},"budget":{"required":true}
                },
                {
                    "id":"history","enabled":true,"requestedRole":"context",
                    "source":{"type":"history","bindingId":"history","strategy":{"type":"all"}},
                    "position":{"type":"history"},"budget":{"required":false},
                    "overflow":{"type":"keep_recent","count":null}
                }
            ]}}),
            &[("idempotency-key", "template-preset-publish".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let body = json!({"name":"Alice Agent","channelId":channel_id,"presetId":preset_id});
    let revision = call(
        &app,
        request(
            "POST",
            "/v1/roleplay/templates",
            body.clone(),
            &[("idempotency-key", "template-create".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    call(
        &app,
        request(
            "POST",
            &format!("/v1/channels/{channel_id}/revisions"),
            json!({"expectedHeadRevisionId":first_channel_revision["id"],"spec":{
                "operationTaxonomyVersion":1,"adapterDecoderVersion":1,
                "baseUrl":"https://llm-v2.example.test/v1",
                "transportPolicy":{"allowLoopbackHttp":false,"allowUnauthenticated":true},
                "credential":{"type":"none"},
                "operationKeys":[{"operation":"generate_content","kind":"open_ai_responses"}],
                "modelCatalogs":[{"operationKey":{"operation":"generate_content","kind":"open_ai_responses"},"policy":"allowlist","models":[{"id":"replacement-model","capabilities":{"structuredOutput":true}}]}]
            }}),
            &[("idempotency-key", "template-channel-publish-v2".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let replay = call(
        &app,
        request(
            "POST",
            "/v1/roleplay/templates",
            body,
            &[("idempotency-key", "template-create".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(revision["id"], replay["id"]);
    let compatibility = call(
        &app,
        request(
            "GET",
            &format!(
                "/v1/graph-revisions/{}/roleplay-compatibility",
                revision["id"].as_str().unwrap()
            ),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(compatibility["mode"], "editable");
    let settings = call(
        &app,
        request(
            "GET",
            &format!(
                "/v1/graph-revisions/{}/roleplay-settings",
                revision["id"].as_str().unwrap()
            ),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(settings["profileVersion"], 1);
    assert_eq!(settings["model"]["modelId"], "role-model");
    assert_eq!(settings["contextPresetId"], preset_id);
    assert_eq!(
        revision["definition"]["nodes"][1]["model"]["modelId"],
        "role-model"
    );
    assert_eq!(
        revision["definition"]["nodes"][1]["memory"]["reads"][0]["source"]["kind"],
        "conversation_history"
    );
}
