use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_storage::SqliteStore;

use super::{
    call,
    context_merge::{commit, fork},
    request, test_app,
};

#[tokio::test]
async fn context_merge_http_applies_an_explicit_resolution() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let conversation = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({}),
            &[("idempotency-key", "merge-resolution-conversation".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let context = conversation["contextId"].as_str().unwrap();
    let root_branch = conversation["activeBranchId"].as_str().unwrap();
    let root_commit = conversation["activeHeadCommitId"].as_str().unwrap();
    let source = fork(&app, context, root_branch, root_commit, "resolution-source").await;
    let target = fork(&app, context, root_branch, root_commit, "resolution-target").await;
    let source_head = commit(
        &app,
        context,
        source["branchId"].as_str().unwrap(),
        root_commit,
        "resolution-source-change",
        "/value",
        1,
    )
    .await;
    let target_head = commit(
        &app,
        context,
        target["branchId"].as_str().unwrap(),
        root_commit,
        "resolution-target-change",
        "/value",
        2,
    )
    .await;
    let body = json!({
        "sourceBranchId":source["branchId"],
        "targetBranchId":target["branchId"],
        "expectedSourceHead":source_head,
        "expectedTargetHead":target_head,
        "sourceDisposition":"mark_merged"
    });
    let conflicted = call(
        &app,
        request(
            "POST",
            &format!("/v1/contexts/{context}/merges"),
            body.clone(),
            &[("idempotency-key", "merge-resolution-analysis".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(conflicted["status"], "conflicted");
    let conflict = &conflicted["conflicts"][0];
    let mut resolved_body = body;
    resolved_body["selections"] = json!([{
        "conflictId":conflict["conflictId"],
        "path":conflict["path"],
        "resolution":{"type":"final_value","value":3}
    }]);
    let merged = call(
        &app,
        request(
            "POST",
            &format!("/v1/contexts/{context}/merges"),
            resolved_body,
            &[("idempotency-key", "merge-resolution-apply".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(merged["status"], "merged");
    assert!(merged["mergeCommitId"].is_string());
}
