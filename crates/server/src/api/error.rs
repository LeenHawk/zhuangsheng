use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Value, json};
use ulid::Ulid;
use zhuangsheng_core::application::ApplicationError;

#[derive(Debug, Serialize)]
pub struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiErrorBody {
    code: &'static str,
    message: String,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
    trace_id: String,
}

pub struct ApiError {
    status: StatusCode,
    envelope: ApiErrorEnvelope,
}

impl ApiError {
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message, false, None)
    }

    fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        retryable: bool,
        details: Option<Value>,
    ) -> Self {
        Self {
            status,
            envelope: ApiErrorEnvelope {
                error: ApiErrorBody {
                    code,
                    message: message.into(),
                    retryable,
                    details,
                    trace_id: format!("trace_{}", Ulid::new()),
                },
            },
        }
    }
}

impl From<ApplicationError> for ApiError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::InvalidArgument { code, message } => {
                Self::new(StatusCode::BAD_REQUEST, code, message, false, None)
            }
            ApplicationError::Validation { code, issues } => Self::new(
                StatusCode::BAD_REQUEST,
                code,
                "validation failed",
                false,
                Some(json!({ "issues": issues })),
            ),
            ApplicationError::NotFound { .. } => Self::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found",
                false,
                None,
            ),
            ApplicationError::Conflict(code) => {
                Self::new(StatusCode::CONFLICT, code, "resource changed", false, None)
            }
            ApplicationError::IdempotencyConflict => Self::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "idempotency key was already used for a different request",
                false,
                None,
            ),
            ApplicationError::Gone { code, message } => {
                Self::new(StatusCode::GONE, code, message, false, None)
            }
            ApplicationError::Unauthenticated { code, message } => {
                Self::new(StatusCode::UNAUTHORIZED, code, message, false, None)
            }
            ApplicationError::RateLimited { code, message } => {
                Self::new(StatusCode::TOO_MANY_REQUESTS, code, message, true, None)
            }
            ApplicationError::Unavailable => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "storage_unavailable",
                "storage is temporarily unavailable",
                true,
                None,
            ),
            ApplicationError::Internal => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "an internal integrity check failed",
                false,
                None,
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.envelope)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
