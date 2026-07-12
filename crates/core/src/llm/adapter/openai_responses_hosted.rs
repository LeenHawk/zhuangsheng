use serde_json::{Map, Value};

use crate::llm::ir::{HostedToolDescriptorIr, MetadataValue};

use super::ShapeAdapterError;

pub(super) fn encode_hosted_tool(
    tool: &HostedToolDescriptorIr,
) -> Result<Value, ShapeAdapterError> {
    if tool.hosted_kind != "web_search" {
        return error(
            "unsupported_responses_hosted_tool",
            "Responses hosted tool kind has no explicit wire mapping",
        );
    }
    let mut output = Map::from_iter([("type".into(), Value::String("web_search".into()))]);
    for (key, value) in &tool.config {
        if key != "search_context_size" {
            return error(
                "unsupported_responses_hosted_config",
                "Responses hosted tool config is outside the explicit allowlist",
            );
        }
        let MetadataValue::String(value) = value else {
            return error(
                "invalid_responses_hosted_config",
                "Responses search context size must be a string",
            );
        };
        if !matches!(value.as_str(), "low" | "medium" | "high") {
            return error(
                "invalid_responses_hosted_config",
                "Responses search context size is invalid",
            );
        }
        output.insert(key.clone(), Value::String(value.clone()));
    }
    Ok(Value::Object(output))
}

fn error<T>(code: &'static str, message: &'static str) -> Result<T, ShapeAdapterError> {
    Err(ShapeAdapterError::new(code, message))
}
