use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_core::scheduler::Scheduler;
use zhuangsheng_storage::SqliteStore;

use crate::llm_executor::LocalLlmExecutor;

use super::{call, now_ms, request, roleplay_provider::FakeRoleProvider, test_app};

#[tokio::test]
async fn public_roleplay_journey_projects_a_fake_provider_reply() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store.clone());
    let revision_id = configure_roleplay(&app).await;
    let run = json!({
        "graphRevisionId":revision_id,
        "replyOutputKey":"reply",
        "inputShape":"conversation_message_v1"
    });
    let conversation = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({"title":"The Moonlit Archive","defaultRun":run}),
            &[("idempotency-key", "journey-conversation".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let submitted = call(
        &app,
        request(
            "POST",
            &format!(
                "/v1/conversations/{}/turns",
                conversation["id"].as_str().unwrap()
            ),
            json!({
                "expectedHeadCommitId":conversation["activeHeadCommitId"],
                "userContent":[{"type":"text","text":"Open the moonlit archive."}],
                "run":run
            }),
            &[("idempotency-key", "journey-turn".into())],
        ),
        StatusCode::ACCEPTED,
    )
    .await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(FakeRoleProvider::new(requests.clone())),
    ));
    Scheduler::new(store.clone(), "journey-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store
            .maintain_candidate_projections(now_ms(), "journey-projector", 10)
            .await
            .unwrap(),
        1
    );
    let timeline = call(
        &app,
        request(
            "GET",
            &format!(
                "/v1/conversations/{}/turns",
                conversation["id"].as_str().unwrap()
            ),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(timeline["turns"][0]["candidates"][0]["status"], "ready");
    assert_eq!(
        timeline["turns"][0]["selectedRunId"],
        submitted["run"]["id"]
    );
    assert_eq!(timeline["messages"][1]["role"], "assistant");
    assert_eq!(
        timeline["messages"][1]["content"][0]["text"],
        "The archive remembers you."
    );

    let captured = requests.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let user_messages: Vec<_> = captured[0]["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["role"] == "user")
        .collect();
    assert_eq!(user_messages.len(), 1);
    assert_eq!(
        user_messages[0]["content"][0]["text"],
        "Open the moonlit archive."
    );
}

async fn configure_roleplay(app: &axum::Router) -> String {
    let channel = call(
        app,
        request(
            "POST",
            "/v1/channels",
            json!({"name":"Role model"}),
            &[("idempotency-key", "journey-channel".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let channel_id = channel["id"].as_str().unwrap();
    call(
        app,
        request("POST", &format!("/v1/channels/{channel_id}/revisions"), json!({"expectedHeadRevisionId":null,"spec":{
            "operationTaxonomyVersion":1,"adapterDecoderVersion":1,
            "baseUrl":"https://fake.example.test/v1",
            "transportPolicy":{"allowLoopbackHttp":false,"allowUnauthenticated":true},
            "credential":{"type":"none"},
            "operationKeys":[{"operation":"generate_content","kind":"open_ai_responses"}],
            "modelCatalogs":[{"operationKey":{"operation":"generate_content","kind":"open_ai_responses"},"policy":"allowlist","models":[{"id":"role-model","capabilities":{"structuredOutput":true}}]}]
        }}), &[("idempotency-key", "journey-channel-revision".into())]),
        StatusCode::CREATED,
    ).await;
    let preset = call(
        app,
        request(
            "POST",
            "/v1/context-presets",
            json!({"name":"Alice"}),
            &[("idempotency-key", "journey-preset".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let preset_id = preset["id"].as_str().unwrap();
    call(
        app,
        request("POST", &format!("/v1/context-presets/{preset_id}/revisions"), json!({"expectedHeadVersionId":null,"spec":{"mode":"chat","items":[
            {"id":"character","enabled":true,"requestedRole":"system","source":{"type":"literal","text":"You are Alice."},"position":{"type":"start"},"budget":{"required":true}},
            {"id":"history","enabled":true,"requestedRole":"context","source":{"type":"history","bindingId":"history","strategy":{"type":"all"}},"position":{"type":"history"},"overflow":{"type":"keep_recent","count":null}}
        ]}}), &[("idempotency-key", "journey-preset-version".into())]),
        StatusCode::CREATED,
    ).await;
    call(
        app,
        request(
            "POST",
            "/v1/roleplay/templates",
            json!({"name":"Alice Agent","channelId":channel_id,"presetId":preset_id}),
            &[("idempotency-key", "journey-template".into())],
        ),
        StatusCode::CREATED,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
