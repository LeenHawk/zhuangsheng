use thiserror::Error;
use zhuangsheng_core::DomainError;

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Error)]
pub enum StorageError {
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
    Database(#[from] sea_orm::DbErr),
}
