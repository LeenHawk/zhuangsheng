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

    let body = json!({
        "document":preset_document(),
        "sourceName":"Roleplay.json",
        "targetPresetId":null,
        "expectedHeadVersionId":null
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
    assert_eq!(
        first["version"]["spec"]["textTransforms"][0]["id"],
        "clean-output"
    );
}

fn preset_document() -> Value {
    json!({
        "name":"Imported Roleplay",
        "temperature":0.8,
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
