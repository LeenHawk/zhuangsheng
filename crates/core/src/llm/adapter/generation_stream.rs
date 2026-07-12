use crate::llm::LlmOperationExecutionPin;

use super::{
    ClaudeStreamDecoder, DecodedStreamBatch, GeminiStreamDecoder, OpenAiChatStreamDecoder,
    OpenAiResponsesStreamDecoder, ShapeAdapterError, ShapeAdapterKey, resolve_shape_adapter,
};

pub enum GenerationStreamDecoder {
    OpenAiResponses(OpenAiResponsesStreamDecoder),
    OpenAiChat(OpenAiChatStreamDecoder),
    Claude(ClaudeStreamDecoder),
    Gemini(GeminiStreamDecoder),
}

impl GenerationStreamDecoder {
    pub fn new(
        pin: LlmOperationExecutionPin,
        model_call_id: impl Into<String>,
    ) -> Result<Self, ShapeAdapterError> {
        let model_call_id = model_call_id.into();
        Ok(match resolve_shape_adapter(&pin)?.key {
            ShapeAdapterKey::OpenAiResponsesV1 => {
                Self::OpenAiResponses(OpenAiResponsesStreamDecoder::new(pin, model_call_id)?)
            }
            ShapeAdapterKey::OpenAiChatCompletionsV1 => {
                Self::OpenAiChat(OpenAiChatStreamDecoder::new(pin, model_call_id)?)
            }
            ShapeAdapterKey::ClaudeMessagesV1 => {
                Self::Claude(ClaudeStreamDecoder::new(pin, model_call_id)?)
            }
            ShapeAdapterKey::GeminiGenerateContentV1 => {
                Self::Gemini(GeminiStreamDecoder::new(pin, model_call_id)?)
            }
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        match self {
            Self::OpenAiResponses(decoder) => decoder.push(bytes),
            Self::OpenAiChat(decoder) => decoder.push(bytes),
            Self::Claude(decoder) => decoder.push(bytes),
            Self::Gemini(decoder) => decoder.push(bytes),
        }
    }

    pub fn finish(&mut self) -> Result<DecodedStreamBatch, ShapeAdapterError> {
        match self {
            Self::OpenAiResponses(_) => Ok(DecodedStreamBatch::default()),
            Self::OpenAiChat(decoder) => decoder.finish(),
            Self::Claude(decoder) => {
                decoder.finish()?;
                Ok(DecodedStreamBatch::default())
            }
            Self::Gemini(decoder) => {
                decoder.finish()?;
                Ok(DecodedStreamBatch::default())
            }
        }
    }
}
