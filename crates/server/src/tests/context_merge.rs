use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{Value, json};
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn context_merge_http_merges_disjoint_fork_changes() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let conversation = call(
        &app,
        request(
            "POST",
            "/v1/conversations",
            json!({}),
            &[("idempotency-key", "merge-http-conversation".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let context = conversation["contextId"].as_str().unwrap();
    let root_branch = conversation["activeBranchId"].as_str().unwrap();
    let root_commit = conversation["activeHeadCommitId"].as_str().unwrap();
    let source = fork(&app, context, root_branch, root_commit, "merge-http-source").await;
    let target = fork(&app, context, root_branch, root_commit, "merge-http-target").await;
    let source_head = commit(
        &app,
        context,
        source["branchId"].as_str().unwrap(),
        root_commit,
        "merge-http-source-change",
        "/source",
        1,
    )
    .await;
    let target_head = commit(
        &app,
        context,
        target["branchId"].as_str().unwrap(),
        root_commit,
        "merge-http-target-change",
        "/target",
        2,
    )
    .await;
    let merged = call(
        &app,
        request(
            "POST",
            &format!("/v1/contexts/{context}/merges"),
            json!({
                "sourceBranchId":source["branchId"],
                "targetBranchId":target["branchId"],
                "expectedSourceHead":source_head,
                "expectedTargetHead":target_head,
                "sourceDisposition":"keep_active",
                "selections":[]
            }),
            &[("idempotency-key", "merge-http".into())],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(merged["status"], "merged");
    let projection = call(
        &app,
        request(
            "GET",
            &format!(
                "/v1/contexts/{context}/branches/{}",
                target["branchId"].as_str().unwrap()
            ),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(projection["value"]["source"], 1);
    assert_eq!(projection["value"]["target"], 2);
    let commits = call(
        &app,
        request(
            "GET",
            &format!("/v1/contexts/{context}/commits"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    let source_commit = commits
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["operationId"] == "merge-http-source-change")
        .unwrap();
    assert_eq!(source_commit["author"]["kind"], "user");
    assert_eq!(source_commit["author"]["id"], "local-user");
}

pub(super) async fn fork(
    app: &axum::Router,
    context: &str,
    branch: &str,
    commit: &str,
    key: &str,
) -> Value {
    call(
        app,
        request(
            "POST",
            &format!("/v1/contexts/{context}/branches"),
            json!({"sourceBranchId":branch,"fromCommitId":commit,"expectedSourceHead":commit}),
            &[("idempotency-key", key.into())],
        ),
        StatusCode::CREATED,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn commit(
    app: &axum::Router,
    context: &str,
    branch: &str,
    base: &str,
    operation: &str,
    path: &str,
    value: i64,
) -> String {
    call(
        app,
        request(
            "POST",
            &format!("/v1/contexts/{context}/branches/{branch}/commits"),
            json!({
                "patch":{
                    "aggregateKind":"working_context","aggregateId":context,"lineageKey":branch,
                    "baseCommitId":base,"operationId":operation,
                    "ops":[{"op":"add","path":path,"value":value}],
                    "schemaVersion":1,"policyVersion":1,"author":{"kind":"node","id":"forged-node"}
                },
                "originRunId":null,"originNodeInstanceId":null
            }),
            &[],
        ),
        StatusCode::CREATED,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .into()
}
