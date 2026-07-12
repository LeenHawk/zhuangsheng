use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_core::runtime::{RunContextCommand, StartRunCommand};
use zhuangsheng_storage::SqliteStore;

use super::{call, conversation_profile::compatible_revision, request, test_app};

#[tokio::test]
async fn recent_runs_http_returns_bounded_run_views() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = compatible_revision(&store).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({
                "schemaVersion":1,
                "conversationId":"conversation_1",
                "turnId":"turn_1",
                "userMessageId":"message_1",
                "userCommitId":"commit_1",
                "content":[{"type":"text","text":"hello"}]
            }),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "recent-run".into(),
        })
        .await
        .unwrap();
    let app = test_app(store);

    let listed = call(
        &app,
        request("GET", "/v1/runs?limit=1", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], run.id);
    assert_eq!(listed["items"][0]["status"], "running");
}
