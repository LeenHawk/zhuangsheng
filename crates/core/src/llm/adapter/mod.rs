mod claude;
mod claude_stream;
mod common;
mod count;
#[cfg(test)]
mod count_tests;
mod gemini;
mod gemini_stream;
mod generation;
mod generation_stream;
mod openai_chat;
mod openai_chat_stream;
mod openai_responses;
mod openai_responses_hosted;
mod openai_responses_opaque;
mod openai_responses_stream;
mod registry;
#[cfg(test)]
mod stream_tests;
#[cfg(test)]
mod tests;
mod types;

pub use claude::{decode_claude_terminal, encode_claude_request};
pub use claude_stream::ClaudeStreamDecoder;
pub use count::{decode_count_terminal, encode_count_request};
pub use gemini::{decode_gemini_terminal, encode_gemini_request};
pub use gemini_stream::GeminiStreamDecoder;
pub use generation::{
    decode_generation_terminal, encode_generation_request, restore_generation_request,
};
pub use generation_stream::GenerationStreamDecoder;
pub use openai_chat::{decode_openai_chat_terminal, encode_openai_chat_request};
pub use openai_chat_stream::OpenAiChatStreamDecoder;
pub use openai_responses::{decode_openai_responses_terminal, encode_openai_responses_request};
pub use openai_responses_stream::OpenAiResponsesStreamDecoder;
pub use registry::{ShapeAdapterDescriptor, resolve_shape_adapter};
pub use types::*;
