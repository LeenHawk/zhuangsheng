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

use crate::{AppServices, StreamEventHub, app};

mod artifact;
mod config;
mod context_fork;
mod context_merge;
mod context_merge_resolution;
mod conversation;
mod conversation_profile;
mod conversation_projection_resolution;
mod conversation_turn;
mod secret;

fn test_app(store: Arc<SqliteStore>) -> axum::Router {
    app(AppServices {
        artifact: store.clone(),
        graph: store.clone(),
        channel: store.clone(),
        preset: store.clone(),
        context: store.clone(),
        conversation: store.clone(),
        memory: store.clone(),
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
    Scheduler::new(store, "http-test-worker")
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
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/runs/{run_id}/events"),
            json!(null),
            &[("last-event-id", (last_seq - 1).to_string())],
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
    assert!(data.contains(&format!("id: {last_seq}")));
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
async fn wait_response_route_uses_typed_blocker_decisions() {
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
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(response["error"]["code"], "unsupported_wait_response");
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
    assert_eq!(response.status(), expected);
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
