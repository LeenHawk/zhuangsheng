use thiserror::Error;
use zhuangsheng_core::DomainError;
use zhuangsheng_core::llm::LlmConfigError;
use zhuangsheng_core::memory::MemoryValidationError;
use zhuangsheng_core::state::StatePatchError;

use crate::secret::SecretStoreError;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("run input contract violation: {0}")]
    InputContract(String),
    #[error("resource not found: {kind} {id}")]
    NotFound { kind: &'static str, id: String },
    #[error("optimistic concurrency conflict: {0}")]
    Conflict(&'static str),
    #[error("idempotency key conflicts with another request")]
    IdempotencyConflict,
    #[error("stored data failed integrity validation: {0}")]
    Integrity(String),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    StatePatch(#[from] StatePatchError),
    #[error(transparent)]
    MemoryValidation(#[from] MemoryValidationError),
    #[error(transparent)]
    LlmConfig(#[from] LlmConfigError),
    #[error(transparent)]
    SecretStore(#[from] SecretStoreError),
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<StorageError> for zhuangsheng_core::application::ApplicationError {
    fn from(error: StorageError) -> Self {
        use zhuangsheng_core::{DomainError, application::ApplicationError};

        match error {
            StorageError::InvalidArgument(message) => ApplicationError::InvalidArgument {
                code: "invalid_argument",
                message,
            },
            StorageError::InputContract(message) => ApplicationError::InvalidArgument {
                code: "input_contract_violation",
                message,
            },
            StorageError::NotFound { kind, id } => ApplicationError::NotFound { kind, id },
            StorageError::Conflict(code) => ApplicationError::Conflict(code),
            StorageError::IdempotencyConflict => ApplicationError::IdempotencyConflict,
            StorageError::Domain(DomainError::GraphValidation(issues)) => {
                ApplicationError::Validation {
                    code: "graph_validation_failed",
                    issues,
                }
            }
            StorageError::Domain(DomainError::SchemaValidation(issues)) => {
                ApplicationError::Validation {
                    code: "schema_validation_failed",
                    issues,
                }
            }
            StorageError::Domain(DomainError::InvalidJson(message))
            | StorageError::Domain(DomainError::JsonLimit(message)) => {
                ApplicationError::InvalidArgument {
                    code: "invalid_json",
                    message,
                }
            }
            StorageError::Database(_) | StorageError::Io(_) => ApplicationError::Unavailable,
            StorageError::StatePatch(error) => ApplicationError::InvalidArgument {
                code: error.code,
                message: error.message,
            },
            StorageError::MemoryValidation(error) => ApplicationError::InvalidArgument {
                code: error.code,
                message: error.message,
            },
            StorageError::LlmConfig(error) => ApplicationError::InvalidArgument {
                code: error.code,
                message: error.message,
            },
            StorageError::SecretStore(error) => {
                let code = error.code();
                match error {
                    SecretStoreError::NotInitialized
                    | SecretStoreError::AlreadyInitialized
                    | SecretStoreError::Locked
                    | SecretStoreError::SessionExpired => ApplicationError::Conflict(code),
                    SecretStoreError::NotFound(id) => {
                        ApplicationError::NotFound { kind: "secret", id }
                    }
                    SecretStoreError::UnlockFailed => ApplicationError::Unauthenticated {
                        code,
                        message: "secret store unlock failed".into(),
                    },
                    SecretStoreError::IdempotencyKeyExpired => ApplicationError::Gone {
                        code,
                        message: "idempotency result is no longer valid".into(),
                    },
                    SecretStoreError::InvalidArgument(message) => {
                        ApplicationError::InvalidArgument { code, message }
                    }
                    SecretStoreError::CorruptStore
                    | SecretStoreError::UnsupportedFormat
                    | SecretStoreError::Crypto => ApplicationError::Internal,
                    SecretStoreError::RateLimited => ApplicationError::RateLimited {
                        code,
                        message: "too many secret store unlock attempts".into(),
                    },
                }
            }
            StorageError::Integrity(_) | StorageError::Domain(DomainError::Serialization(_)) => {
                ApplicationError::Internal
            }
        }
    }
}
