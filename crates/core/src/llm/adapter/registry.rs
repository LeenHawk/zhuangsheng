use super::types::{ShapeAdapterError, ShapeAdapterKey};
use crate::{
    compatibility::supports_operation_versions,
    llm::{ContentGenerationKind, LlmOperationExecutionPin, OperationKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeAdapterDescriptor {
    pub key: ShapeAdapterKey,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub supports_streaming: bool,
    pub supports_same_shape_tool_roundtrip: bool,
}

pub fn resolve_shape_adapter(
    pin: &LlmOperationExecutionPin,
) -> Result<ShapeAdapterDescriptor, ShapeAdapterError> {
    if !supports_operation_versions(pin.operation_taxonomy_version, pin.adapter_decoder_version) {
        return Err(ShapeAdapterError::new(
            "unsupported_operation_version",
            "execution pin uses an unsupported taxonomy/decoder pair",
        ));
    }
    if pin.channel_revision_id.is_empty() || pin.model_id.is_empty() {
        return Err(ShapeAdapterError::new(
            "invalid_execution_pin",
            "channel revision and model id are required",
        ));
    }
    let OperationKind::ContentGeneration(kind) = pin.operation_key.kind else {
        return Err(ShapeAdapterError::new(
            "unsupported_generation_operation",
            "generation adapter received a provider-scoped operation",
        ));
    };
    if !pin.operation_key.operation.is_content_generation() {
        return Err(ShapeAdapterError::new(
            "unsupported_generation_operation",
            "operation is not content generation",
        ));
    }
    let key = match kind {
        ContentGenerationKind::OpenAiResponses => ShapeAdapterKey::OpenAiResponsesV1,
        ContentGenerationKind::OpenAiResponsesWebSocket => {
            return Err(ShapeAdapterError::new(
                "unsupported_generation_transport",
                "OpenAI Responses WebSocket transport is not implemented",
            ));
        }
        ContentGenerationKind::OpenAiChatCompletions => ShapeAdapterKey::OpenAiChatCompletionsV1,
        ContentGenerationKind::ClaudeMessages => ShapeAdapterKey::ClaudeMessagesV1,
        ContentGenerationKind::GeminiGenerateContent => ShapeAdapterKey::GeminiGenerateContentV1,
    };
    Ok(ShapeAdapterDescriptor {
        key,
        operation_taxonomy_version: pin.operation_taxonomy_version,
        adapter_decoder_version: pin.adapter_decoder_version,
        supports_streaming: true,
        supports_same_shape_tool_roundtrip: true,
    })
}

#[cfg(test)]
mod tests {
    use crate::llm::{ContentGenerationKind, Operation, OperationKey};

    use super::*;

    #[test]
    fn exact_registry_resolves_all_four_generation_shapes() {
        let kinds = [
            (
                ContentGenerationKind::OpenAiResponses,
                ShapeAdapterKey::OpenAiResponsesV1,
            ),
            (
                ContentGenerationKind::OpenAiChatCompletions,
                ShapeAdapterKey::OpenAiChatCompletionsV1,
            ),
            (
                ContentGenerationKind::ClaudeMessages,
                ShapeAdapterKey::ClaudeMessagesV1,
            ),
            (
                ContentGenerationKind::GeminiGenerateContent,
                ShapeAdapterKey::GeminiGenerateContentV1,
            ),
        ];
        for (kind, expected) in kinds {
            let pin = LlmOperationExecutionPin {
                channel_revision_id: "revision-1".into(),
                model_id: "model-1".into(),
                operation_key: OperationKey::content_generation(Operation::GenerateContent, kind),
                operation_taxonomy_version: 1,
                adapter_decoder_version: 1,
            };
            assert_eq!(resolve_shape_adapter(&pin).unwrap().key, expected);
        }
    }

    #[test]
    fn exact_registry_rejects_unknown_decoder_before_shape_use() {
        let pin = LlmOperationExecutionPin {
            channel_revision_id: "revision-1".into(),
            model_id: "model-1".into(),
            operation_key: OperationKey::content_generation(
                Operation::GenerateContent,
                ContentGenerationKind::OpenAiResponses,
            ),
            operation_taxonomy_version: 1,
            adapter_decoder_version: 99,
        };
        assert_eq!(
            resolve_shape_adapter(&pin).unwrap_err().code,
            "unsupported_operation_version"
        );
    }

    #[test]
    fn websocket_responses_requires_a_dedicated_transport() {
        let pin = LlmOperationExecutionPin {
            channel_revision_id: "revision-1".into(),
            model_id: "model-1".into(),
            operation_key: OperationKey::content_generation(
                Operation::GenerateContent,
                ContentGenerationKind::OpenAiResponsesWebSocket,
            ),
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
        };
        assert_eq!(
            resolve_shape_adapter(&pin).unwrap_err().code,
            "unsupported_generation_transport"
        );
    }
}
