use thiserror::Error;

pub type LlmConfigResult<T> = Result<T, LlmConfigError>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct LlmConfigError {
    pub code: &'static str,
    pub message: String,
}

impl LlmConfigError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
