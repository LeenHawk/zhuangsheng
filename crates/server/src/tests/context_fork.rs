use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn context_fork_http_creates_a_historical_branch_idempotently() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let conversation = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({}),
            &[("idempotency-key", "fork-http-conversation".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let context_id = conversation["contextId"].as_str().unwrap();
    let source_branch_id = conversation["activeBranchId"].as_str().unwrap();
    let root_commit_id = conversation["activeHeadCommitId"].as_str().unwrap();
    let body = json!({
        "sourceBranchId":source_branch_id,
        "fromCommitId":root_commit_id,
        "expectedSourceHead":root_commit_id
    });
    let branch = call(
        &app,
        request(
            "POST",
            &format!("/v1/contexts/{context_id}/branches"),
            body.clone(),
            &[("idempotency-key", "fork-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(branch["forkCommitId"], root_commit_id);
    assert_ne!(branch["branchId"], source_branch_id);
    let replayed = call(
        &app,
        request(
            "POST",
            &format!("/v1/contexts/{context_id}/branches"),
            body,
            &[("idempotency-key", "fork-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(replayed, branch);
    let projection = call(
        &app,
        request(
            "GET",
            &format!(
                "/v1/contexts/{context_id}/branches/{}",
                branch["branchId"].as_str().unwrap()
            ),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        projection["value"],
        json!({"schemaVersion":1,"messages":[]})
    );
}
