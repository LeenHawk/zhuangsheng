use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type DomainResult<T> = Result<T, DomainError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

impl ValidationIssue {
    pub fn error(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("canonical JSON limit exceeded: {0}")]
    JsonLimit(String),
    #[error("graph validation failed")]
    GraphValidation(Vec<ValidationIssue>),
    #[error("schema validation failed")]
    SchemaValidation(Vec<ValidationIssue>),
    #[error("serialization failed: {0}")]
    Serialization(String),
}
