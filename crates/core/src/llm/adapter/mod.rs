mod common;
mod openai_chat;
mod registry;
mod types;

pub use openai_chat::{decode_openai_chat_terminal, encode_openai_chat_request};
pub use registry::{ShapeAdapterDescriptor, resolve_shape_adapter};
pub use types::*;
