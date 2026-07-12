use serde::Serialize;
use serde_json::Value;
use zhuangsheng_core::application::ApplicationError;

pub type CommandResult<T> = Result<T, TauriCommandError>;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TauriCommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl From<ApplicationError> for TauriCommandError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::InvalidArgument { code, message } => Self::new(code, message, false),
            ApplicationError::Validation { code, issues } => Self {
                code: code.into(),
                message: "validation failed".into(),
                retryable: false,
                details: Some(serde_json::json!({ "issues": issues })),
            },
            ApplicationError::NotFound { .. } => {
                Self::new("not_found", "resource not found", false)
            }
            ApplicationError::Conflict(code) => Self::new(code, "resource changed", false),
            ApplicationError::IdempotencyConflict => Self::new(
                "idempotency_conflict",
                "idempotency key conflicts with another request",
                false,
            ),
            ApplicationError::Gone { code, message } => Self::new(code, message, false),
            ApplicationError::Unauthenticated { code, message } => Self::new(code, message, false),
            ApplicationError::RateLimited { code, message } => Self::new(code, message, true),
            ApplicationError::Unavailable => Self::new(
                "storage_unavailable",
                "storage is temporarily unavailable",
                true,
            ),
            ApplicationError::Internal => {
                Self::new("internal_error", "internal integrity check failed", false)
            }
        }
    }
}

impl TauriCommandError {
    fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
            details: None,
        }
    }
}
