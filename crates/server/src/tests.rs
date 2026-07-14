use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zhuangsheng_core::scheduler::Scheduler;
use zhuangsheng_storage::SqliteStore;

use crate::{
    AppServices, RemoteModelDiscoveryService, StreamEventHub, app, provider::HttpProviderClient,
};

mod artifact;
mod config;
mod context_fork;
mod context_merge;
mod context_merge_resolution;
mod conversation;
mod conversation_profile;
mod conversation_projection_resolution;
mod conversation_turn;
mod effect_resolution;
mod model_discovery;
mod plugin;
mod plugin_support;
mod roleplay_compatibility;
mod roleplay_journey;
mod roleplay_provider;
mod roleplay_template;
mod run_list;
mod secret;
mod wait_response;

fn test_app(store: Arc<SqliteStore>) -> axum::Router {
    let model_discovery = Arc::new(RemoteModelDiscoveryService::new(
        store.clone(),
        store.clone(),
        Arc::new(HttpProviderClient::new().unwrap()),
    ));
    test_app_with_discovery(store, model_discovery)
}

fn test_app_with_discovery(
    store: Arc<SqliteStore>,
    model_discovery: Arc<dyn zhuangsheng_core::application::channel::ChannelModelDiscoveryService>,
) -> axum::Router {
    app(AppServices {
        artifact: store.clone(),
        graph: store.clone(),
        channel: store.clone(),
        model_discovery,
        preset: store.clone(),
        context: store.clone(),
        conversation: store.clone(),
        memory: store.clone(),
        plugin: plugin_support::service(),
        runtime: store.clone(),
        secret: store.clone(),
        tool_registry: store,
        stream_events: StreamEventHub::new(),
    })
}

#[tokio::test]
async fn graph_http_vertical_slice_uses_public_contract() {
    let store = SqliteStore::connect("sqlite::memory:").await.unwrap();
    let store = Arc::new(store);
    let app = test_app(store.clone());
    let created = call(
        &app,
        request(
            "POST",
            "/v1/graphs",
            json!({"name":"HTTP Graph"}),
            &[("idempotency-key", "create-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let graph_id = created["graph"]["id"].as_str().unwrap();
    let token = created["draftRevisionToken"].as_str().unwrap();

    let draft = call(
        &app,
        request(
            "GET",
            &format!("/v1/graphs/{graph_id}/draft"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(draft["revisionToken"], token);

    let document = json!({
        "graphId": graph_id,
        "name": "HTTP Graph",
        "nodes": [
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges": [{
            "from":{"nodeId":"input","output":"default"},
            "to":{"nodeId":"output","input":"default"}
        }],
        "runInputSchema": null,
        "outputContract": [{"key":"reply","schema":null,"collection":"single","required":true}],
        "limits": null
    });
    let updated = call(
        &app,
        request(
            "PUT",
            &format!("/v1/graphs/{graph_id}/draft"),
            document,
            &[
                ("idempotency-key", "draft-http".into()),
                ("if-match", format!("\"{token}\"")),
            ],
        ),
        StatusCode::OK,
    )
    .await;
    let next_token = updated["revisionToken"].as_str().unwrap();

    let revision = call(
        &app,
        request(
            "POST",
            &format!("/v1/graphs/{graph_id}/apply"),
            json!({"operationTaxonomyVersion":1,"adapterDecoderVersion":1}),
            &[
                ("idempotency-key", "apply-http".into()),
                ("if-match", next_token.into()),
            ],
        ),
        StatusCode::OK,
    )
    .await;
    let revision_id = revision["id"].as_str().unwrap();
    let loaded = call(
        &app,
        request(
            "GET",
            &format!("/v1/graph-revisions/{revision_id}"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(loaded["contentHash"], revision["contentHash"]);

    let run = call(
        &app,
        request(
            "POST",
            &format!("/v1/graphs/{revision_id}/runs"),
            json!({"input":{"message":"hello"},"context":{"mode":"temporary"}}),
            &[("idempotency-key", "run-http".into())],
        ),
        StatusCode::ACCEPTED,
    )
    .await;
    let run_id = run["id"].as_str().unwrap();
    let interrupted = call(
        &app,
        request(
            "POST",
            &format!("/v1/runs/{run_id}/interrupt"),
            json!({"expectedEpoch":0}),
            &[("idempotency-key", "interrupt-http".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(interrupted["status"], "interrupted");
    let resumed = call(
        &app,
        request(
            "POST",
            &format!("/v1/runs/{run_id}/resume"),
            json!({"expectedEpoch":1}),
            &[("idempotency-key", "resume-http".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(resumed["status"], "running");
    Scheduler::new(store.clone(), "http-test-worker")
        .run_until_idle(now_ms(), 64)
        .await
        .unwrap();
    let loaded_run = call(
        &app,
        request("GET", &format!("/v1/runs/{run_id}"), json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(loaded_run["id"], run["id"]);
    assert_eq!(loaded_run["status"], "completed");
    let outputs = call(
        &app,
        request(
            "GET",
            &format!("/v1/runs/{run_id}/outputs"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        outputs["reply"]["values"][0]["value"],
        json!({"message":"hello"})
    );

    let last_seq = loaded_run["lastDurableSeq"].as_u64().unwrap();
    let terminal_seq = store
        .list_run_events(run_id, 0, 500)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.event_type == "run.completed")
        .unwrap()
        .durable_seq;
    assert!(last_seq >= terminal_seq);
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/runs/{run_id}/events"),
            json!(null),
            &[("last-event-id", (terminal_seq - 1).to_string())],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let data = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(data.contains(&format!("id: {terminal_seq}")));
    assert!(data.contains("event: run.completed"));
}

#[tokio::test]
async fn graph_http_errors_use_typed_envelope() {
    let store = SqliteStore::connect("sqlite::memory:").await.unwrap();
    let store = Arc::new(store);
    let app = test_app(store);
    let response = app
        .oneshot(request(
            "POST",
            "/v1/graphs",
            json!({"name":"Missing Key"}),
            &[],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["error"]["code"], "missing_header");
    assert!(
        value["error"]["traceId"]
            .as_str()
            .unwrap()
            .starts_with("trace_")
    );
}

#[tokio::test]
async fn memory_http_flow_uses_service_contract_and_typed_commands() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let proposed = call(
        &app,
        request(
            "POST",
            "/v1/memory-proposals",
            json!({
                "scopeId":"roleplay",
                "memoryId":null,
                "expectedHeadCommitId":null,
                "change":{
                    "type":"create",
                    "content":{
                        "schemaVersion":1,
                        "text":"The moon gate opens at midnight",
                        "tags":["lore"],
                        "attributes":{}
                    }
                },
                "reason":"persist story lore",
                "evidenceRefs":["message:1"],
                "requestedBy":{"kind":"user","id":"user-1"},
                "schemaVersion":1,
                "policyVersion":1,
                "originRunId":null,
                "originNodeInstanceId":null
            }),
            &[("idempotency-key", "memory-propose-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let proposal_id = proposed["id"].as_str().unwrap();
    let memory_id = proposed["memoryId"].as_str().unwrap();
    assert_eq!(proposed["requestedBy"]["kind"], "user");
    assert_eq!(proposed["requestedBy"]["id"], "local-user");
    let inbox = call(
        &app,
        request(
            "GET",
            "/v1/memory-proposals?scopeId=roleplay&status=awaiting_review&limit=10",
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(inbox["proposals"][0]["id"], proposal_id);
    assert_eq!(
        inbox["proposals"][0]["proposedContent"]["text"],
        "The moon gate opens at midnight"
    );
    assert!(inbox["nextCursor"].is_null());
    call(
        &app,
        request(
            "POST",
            &format!("/v1/memory-proposals/{proposal_id}/decision"),
            json!({
                "expectedStatus":"awaiting_review",
                "decision":"approve",
                "actor":{"kind":"user","id":"reviewer"}
            }),
            &[("idempotency-key", "memory-approve-http".into())],
        ),
        StatusCode::OK,
    )
    .await;
    call(
        &app,
        request(
            "POST",
            &format!("/v1/memory-proposals/{proposal_id}/apply"),
            json!({"expectedStatus":"approved"}),
            &[("idempotency-key", "memory-apply-http".into())],
        ),
        StatusCode::OK,
    )
    .await;
    let record = call(
        &app,
        request(
            "GET",
            &format!("/v1/memories/{memory_id}"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(record["status"], "active");
    let results = call(
        &app,
        request(
            "POST",
            "/v1/memory-search",
            json!({
                "scopeId":"roleplay",
                "text":"moon midnight",
                "tags":["lore"],
                "status":"active",
                "limit":10
            }),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(results["records"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn wait_response_route_accepts_typed_memory_proposal_decisions() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let response = call(
        &app,
        request(
            "POST",
            "/v1/waits/wait-1/responses",
            json!({
                "deliveryId":"delivery-1",
                "response":{
                    "type":"blocker_decisions",
                    "decisions":[{
                        "kind":"memory_proposal",
                        "blockerId":"proposal-1",
                        "decision":"approve"
                    }]
                }
            }),
            &[],
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(response["error"]["code"], "not_found");
}

#[tokio::test]
async fn wait_response_route_rejects_mixed_or_open_memory_decisions() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    for (delivery, decisions) in [
        (
            "mixed",
            json!([
                {"kind":"memory_proposal","blockerId":"proposal-1","decision":"approve"},
                {"kind":"tool_call","blockerId":"tool-1","callDigest":"sha256:call","decision":"reject"}
            ]),
        ),
        (
            "open",
            json!([{"kind":"memory_proposal","blockerId":"proposal-1","decision":"approve","unexpected":true}]),
        ),
    ] {
        let response = call(
            &app,
            request(
                "POST",
                "/v1/waits/wait-1/responses",
                json!({
                    "deliveryId":delivery,
                    "response":{"type":"blocker_decisions","decisions":decisions}
                }),
                &[],
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert!(matches!(
            response["error"]["code"].as_str(),
            Some("invalid_wait_response" | "invalid_json_body")
        ));
    }
}

fn request(method: &str, uri: &str, body: Value, headers: &[(&str, String)]) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("accept", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

async fn call(app: &axum::Router, request: Request<Body>, expected: StatusCode) -> Value {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        expected,
        "response body: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
