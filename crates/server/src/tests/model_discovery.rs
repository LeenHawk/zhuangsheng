use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, Operation, adapter::WireGenerationRequest},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    RemoteModelDiscoveryService,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{call, request, test_app_with_discovery};

struct ModelProvider;

#[async_trait]
impl ProviderTransport for ModelProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        assert_eq!(
            wire.operation.operation_key.operation,
            Operation::ListModels
        );
        assert_eq!(wire.relative_path, "/v1/models");
        Ok(ProviderHttpResponse {
            status: 200,
            provider_request_id: Some("models-request".into()),
            body: serde_json::to_vec(&json!({
                "object":"list",
                "data":[
                    {"id":"z-model","object":"model","owned_by":"test"},
                    {"id":"a-model","object":"model","owned_by":"test"}
                ]
            }))
            .unwrap(),
        })
    }
}

#[tokio::test]
async fn channel_model_discovery_uses_pinned_standard_operation() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let discovery = Arc::new(RemoteModelDiscoveryService::new(
        store.clone(),
        store.clone(),
        Arc::new(ModelProvider),
    ));
    let app = test_app_with_discovery(store, discovery);
    let channel = call(
        &app,
        request(
            "POST",
            "/v1/channels",
            json!({"name":"Discoverable"}),
            &[("idempotency-key", "models-channel".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let channel_id = channel["id"].as_str().unwrap();
    let revision = call(
        &app,
        request(
            "POST",
            &format!("/v1/channels/{channel_id}/revisions"),
            json!({"expectedHeadRevisionId":null,"spec":{
                "operationTaxonomyVersion":1,"adapterDecoderVersion":1,
                "baseUrl":"https://models.example.test/v1",
                "transportPolicy":{"allowLoopbackHttp":false,"allowUnauthenticated":true},
                "credential":{"type":"none"},
                "operationKeys":[{"operation":"list_models","kind":"open_ai"}]
            }}),
            &[("idempotency-key", "models-revision".into())],
        ),
        StatusCode::CREATED,
    )
    .await;
    let result = call(
        &app,
        request(
            "POST",
            &format!("/v1/channels/{channel_id}/model-discovery"),
            json!({}),
            &[],
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(result["channelRevisionId"], revision["id"]);
    assert_eq!(result["models"][0]["id"], "a-model");
    assert_eq!(result["models"][1]["id"], "z-model");
}
