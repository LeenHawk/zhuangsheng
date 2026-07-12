use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
};

use crate::provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport};

pub(super) struct FakeRoleProvider {
    requests: Arc<Mutex<Vec<Value>>>,
}

impl FakeRoleProvider {
    pub(super) fn new(requests: Arc<Mutex<Vec<Value>>>) -> Self {
        Self { requests }
    }
}

#[async_trait]
impl ProviderTransport for FakeRoleProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        self.requests
            .lock()
            .unwrap()
            .push(serde_json::from_slice(wire.body()).unwrap());
        Ok(role_response())
    }
}

fn role_response() -> ProviderHttpResponse {
    let reply = r#"{"schemaVersion":1,"type":"assistant_reply","content":[{"type":"text","text":"The archive remembers you."}]}"#;
    ProviderHttpResponse {
        status: 200,
        provider_request_id: Some("journey-request".into()),
        body: serde_json::to_vec(&json!({
            "id":"journey-response","created_at":1,"object":"response","status":"completed",
            "output":[{"type":"message","id":"journey-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":reply,"annotations":[]}]}],
            "usage":{"input_tokens":20,"output_tokens":12,"total_tokens":32,"output_tokens_details":{"reasoning_tokens":0}}
        })).unwrap(),
    }
}
