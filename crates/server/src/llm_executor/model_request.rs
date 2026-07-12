use serde_json::json;
use zhuangsheng_core::{
    application::ApplicationError,
    llm::{LlmOperationExecutionPin, adapter::AdapterExecutionOptions, ir::LlmRequestIr},
};

pub(super) fn durable_generation_request(
    operation: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    options: AdapterExecutionOptions,
) -> Result<Vec<u8>, ApplicationError> {
    zhuangsheng_core::canonical::to_vec(&json!({
        "schemaVersion":1,
        "kind":"generation_request_receipt",
        "operation":operation,
        "stream":options.stream,
        "maxOutputTokens":options.max_output_tokens,
        "request":request,
    }))
    .map_err(|_| ApplicationError::Internal)
}
