use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use zhuangsheng_core::llm::{ContentGenerationKind, Operation, OperationKey};

use crate::provider::ProviderHttpResponse;

pub(super) fn provider_response(text: &str) -> ProviderHttpResponse {
    ProviderHttpResponse {
        status: 200,
        provider_request_id: Some("request-test".into()),
        body: serde_json::to_vec(&json!({
            "id":"response-1",
            "created_at":1,
            "object":"response",
            "output":[{
                "type":"message",
                "id":"message-1",
                "role":"assistant",
                "status":"completed",
                "content":[{"type":"output_text","text":text,"annotations":[]}]
            }],
            "status":"completed",
            "usage":{
                "input_tokens":12,
                "output_tokens":7,
                "total_tokens":19,
                "output_tokens_details":{"reasoning_tokens":0}
            }
        }))
        .unwrap(),
    }
}

pub(super) fn operation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
