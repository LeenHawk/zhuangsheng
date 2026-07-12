use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use zhuangsheng_storage::SqliteStore;

use super::{call, request, test_app};

#[tokio::test]
async fn artifact_http_flow_uploads_commits_and_downloads_safe_content() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let boundary = "zhuangsheng-artifact-boundary";
    let metadata = json!({
        "contextId":null,
        "metadataDraft":{
            "name":"story-note.txt",
            "classification":"private",
            "retention":{"type":"pinned"}
        },
        "declaredMediaType":"text/plain"
    });
    let body = multipart(boundary, &metadata, b"durable role-play note");
    let uploaded = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/v1/artifacts/staging")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap(),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(uploaded["status"], "validated");
    assert!(uploaded.get("validatedContentObjectId").is_none());
    let staging_id = uploaded["stagingId"].as_str().unwrap();
    let artifact = call(
        &app,
        request(
            "POST",
            &format!("/v1/artifacts/staging/{staging_id}/commit"),
            json!({"expectedLifecycleGeneration":uploaded["lifecycleGeneration"]}),
            &[("idempotency-key", "commit-artifact-http".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let artifact_id = artifact["metadata"]["artifactId"].as_str().unwrap();
    assert_eq!(artifact["metadata"]["content"]["mediaType"], "text/plain");
    let loaded = call(
        &app,
        request(
            "GET",
            &format!("/v1/artifacts/{artifact_id}"),
            json!(null),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(loaded, artifact);
    let listed = call(
        &app,
        request("GET", "/v1/artifacts?limit=10", json!(null), &[]),
        StatusCode::OK,
    )
    .await;
    assert_eq!(listed["items"], json!([artifact]));

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/artifacts/{artifact_id}/content"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"story-note.txt\""
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        &b"durable role-play note"[..]
    );
}

#[tokio::test]
async fn artifact_http_rejects_out_of_order_or_mismatched_multipart() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let boundary = "bad-artifact-boundary";
    let metadata = json!({
        "metadataDraft":{
            "name":"image.png",
            "classification":"private",
            "retention":{"type":"pinned"}
        },
        "declaredMediaType":"image/png"
    });
    let response = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/v1/artifacts/staging")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(multipart(boundary, &metadata, b"plain text")))
            .unwrap(),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(response["error"]["code"], "artifact_quarantined");
}

#[tokio::test]
async fn artifact_upload_route_allows_bounded_bodies_above_the_json_limit() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let app = test_app(store);
    let boundary = "large-artifact-boundary";
    let metadata = json!({
        "metadataDraft":{
            "name":"large.txt",
            "classification":"private",
            "retention":{"type":"pinned"}
        },
        "declaredMediaType":"text/plain"
    });
    let object = vec![b'a'; 1024 * 1024 + 128];
    let uploaded = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/v1/artifacts/staging")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(multipart(boundary, &metadata, &object)))
            .unwrap(),
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(uploaded["status"], "validated");
    assert_eq!(uploaded["byteSize"], object.len() as u64);
}

fn multipart(boundary: &str, metadata: &serde_json::Value, object: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n").as_bytes());
    body.extend_from_slice(&serde_json::to_vec(metadata).unwrap());
    body.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"object\"; filename=\"upload\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(object);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}
