use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{Value, json};
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn sillytavern_preview_and_import_share_the_canonical_workflow() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let document = preset_document();
    let preview = call(
        &app,
        request(
            "POST",
            "/v1/compatibility/sillytavern/preview",
            json!({
                "document":document,
                "sourceName":"Roleplay.json",
                "targetPresetId":null
            }),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(preview["kind"], "open_ai");
    assert_eq!(preview["textTransforms"][0]["id"], "clean-output");
    assert_eq!(preview["generation"]["maxOutputTokens"], 512);
    let encoded = serde_json::to_string(&preview).unwrap();
    assert!(!encoded.contains("do-not-leak"));
    assert!(!encoded.contains("secret.invalid"));

    let tested = call(
        &app,
        request(
            "POST",
            "/v1/compatibility/sillytavern/regex/test",
            json!({
                "document":preset_document(), "sourceName":"Roleplay.json",
                "targetPresetId":null, "input":"foo foo", "placement":"ai_output",
                "surface":"canonical", "depth":0, "isEdit":false, "macros":{}
            }),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(tested["text"], "bar bar");
    assert_eq!(tested["appliedRuleIds"], json!(["clean-output"]));

    let channel = call(
        &app,
        request(
            "POST",
            "/v1/channels",
            json!({"name":"ST model"}),
            &[("idempotency-key", "st-channel".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let channel_id = channel["id"].as_str().unwrap();
    call(
        &app,
        request(
            "POST", &format!("/v1/channels/{channel_id}/revisions"),
            json!({"expectedHeadRevisionId":null,"spec":{
                "operationTaxonomyVersion":1,"adapterDecoderVersion":1,
                "baseUrl":"https://llm.example.test/v1",
                "transportPolicy":{"allowLoopbackHttp":false,"allowUnauthenticated":true},
                "credential":{"type":"none"},
                "operationKeys":[{"operation":"generate_content","kind":"open_ai_responses"}],
                "modelCatalogs":[{"operationKey":{"operation":"generate_content","kind":"open_ai_responses"},"policy":"allowlist","models":[{"id":"st-model","capabilities":{"structuredOutput":true}}]}]
            }}),
            &[("idempotency-key", "st-channel-publish".into())],
        ),
        StatusCode::CREATED,
    )
    .await;

    let body = json!({
        "document":preset_document(),
        "sourceName":"Roleplay.json",
        "targetPresetId":null,
        "expectedHeadVersionId":null,
        "channelId":channel_id
    });
    let first = call(
        &app,
        request(
            "POST",
            "/v1/compatibility/sillytavern/import",
            body.clone(),
            &[("idempotency-key", "st-http-import".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let replay = call(
        &app,
        request(
            "POST",
            "/v1/compatibility/sillytavern/import",
            body,
            &[("idempotency-key", "st-http-import".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(first["preset"]["id"], replay["preset"]["id"]);
    assert_eq!(first["version"]["id"], replay["version"]["id"]);
    assert_eq!(first["graphRevision"]["id"], replay["graphRevision"]["id"]);
    assert_eq!(
        first["graphRevision"]["definition"]["nodes"][1]["request"]["generation"]["maxOutputTokens"],
        512
    );
    assert_eq!(
        first["graphRevision"]["definition"]["nodes"][1]["request"]["extensions"]["openai"]["extraBody"]
            ["frequency_penalty"],
        0.2
    );
    assert_eq!(
        first["version"]["spec"]["textTransforms"][0]["id"],
        "clean-output"
    );
    let exported = call(
        &app,
        request(
            "POST",
            "/v1/compatibility/sillytavern/export",
            json!({
                "presetVersionId":first["version"]["id"],
                "graphRevisionId":first["graphRevision"]["id"]
            }),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        exported["bundle"]["documents"][0]["document"]["openai_max_tokens"],
        512
    );
    assert_eq!(
        exported["bundle"]["documents"][0]["document"]["extensions"]["regex_scripts"][0]["id"],
        "clean-output"
    );
    let exported_json = serde_json::to_string(&exported).unwrap();
    assert!(!exported_json.contains("do-not-leak"));
    assert!(!exported_json.contains("secret.invalid"));
}

fn preset_document() -> Value {
    json!({
        "name":"Imported Roleplay",
        "temperature":0.8,
        "frequency_penalty":0.2,
        "openai_max_tokens":512,
        "reverse_proxy":"https://secret.invalid",
        "proxy_password":"do-not-leak",
        "prompts":[
            {"identifier":"main","name":"Main","role":"system","content":"Write a reply."},
            {"identifier":"chatHistory","name":"History","marker":true}
        ],
        "prompt_order":[{"character_id":100001,"order":[
            {"identifier":"main","enabled":true},
            {"identifier":"chatHistory","enabled":true}
        ]}],
        "extensions":{"regex_scripts":[{
            "id":"clean-output","scriptName":"Clean output","findRegex":"/foo/g",
            "replaceString":"bar","trimStrings":[],"placement":[2],"disabled":false,
            "markdownOnly":false,"promptOnly":false,"runOnEdit":false,"substituteRegex":0,
            "minDepth":null,"maxDepth":null
        }]}
    })
}
