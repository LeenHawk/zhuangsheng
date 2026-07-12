use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, conversation_profile::compatible_revision, request, test_app};

#[tokio::test]
async fn conversation_turn_http_returns_one_durable_candidate_run() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = compatible_revision(&store).await;
    let app = test_app(store);
    let conversation = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({}),
            &[("idempotency-key", "turn-http-conversation".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let body = json!({
        "expectedHeadCommitId":conversation["activeHeadCommitId"],
        "userContent":[{"type":"text","text":"Open the archive"}],
        "run":{
            "graphRevisionId":revision_id,
            "replyOutputKey":"reply",
            "inputShape":"conversation_message_v1"
        }
    });
    let submitted = call(
        &app,
        request(
            "POST",
            &format!(
                "/v1/conversations/{}/turns",
                conversation["id"].as_str().unwrap()
            ),
            body.clone(),
            &[("idempotency-key", "turn-http-submit".into())],
        ),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(submitted["candidate"]["runId"], submitted["run"]["id"]);
    assert_eq!(submitted["candidate"]["status"], "running");
    assert_eq!(
        submitted["turn"]["userCommitId"],
        submitted["run"]["inputCommitId"]
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
    assert_eq!(timeline["messages"].as_array().unwrap().len(), 1);
    assert_eq!(timeline["messages"][0]["content"], body["userContent"]);
    assert_eq!(timeline["turns"][0]["candidates"][0]["status"], "running");
    let replayed = call(
        &app,
        request(
            "POST",
            &format!(
                "/v1/conversations/{}/turns",
                conversation["id"].as_str().unwrap()
            ),
            body,
            &[("idempotency-key", "turn-http-submit".into())],
        ),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(replayed, submitted);
    let not_ready = call(
        &app,
        request(
            "PUT",
            &format!(
                "/v1/conversation-turns/{}/selection",
                submitted["turn"]["id"].as_str().unwrap()
            ),
            json!({
                "selectedRunId":submitted["run"]["id"],
                "expectedConversationHeadCommitId":submitted["turn"]["userCommitId"]
            }),
            &[("idempotency-key", "turn-http-select".into())],
        ),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(not_ready["error"]["code"], "candidate_not_ready");
    let regenerated = call(
        &app,
        request(
            "POST",
            &format!(
                "/v1/conversation-turns/{}/candidates",
                submitted["turn"]["id"].as_str().unwrap()
            ),
            json!({
                "expectedUserCommitId":submitted["turn"]["userCommitId"],
                "run":{
                    "graphRevisionId":revision_id,
                    "replyOutputKey":"reply",
                    "inputShape":"conversation_message_v1"
                }
            }),
            &[("idempotency-key", "turn-http-regenerate".into())],
        ),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_ne!(regenerated["run"]["id"], submitted["run"]["id"]);
    assert_eq!(
        regenerated["run"]["inputCommitId"],
        submitted["turn"]["userCommitId"]
    );
}
