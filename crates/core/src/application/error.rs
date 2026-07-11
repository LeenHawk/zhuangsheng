use thiserror::Error;

use crate::ValidationIssue;

#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("invalid argument: {message}")]
    InvalidArgument { code: &'static str, message: String },
    #[error("validation failed")]
    Validation {
        code: &'static str,
        issues: Vec<ValidationIssue>,
    },
    #[error("resource not found: {kind} {id}")]
    NotFound { kind: &'static str, id: String },
    #[error("optimistic concurrency conflict: {0}")]
    Conflict(&'static str),
    #[error("idempotency key conflicts with another request")]
    IdempotencyConflict,
    #[error("storage is temporarily unavailable")]
    Unavailable,
    #[error("internal integrity failure")]
    Internal,
}
