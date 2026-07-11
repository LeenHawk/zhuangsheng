use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret store is not initialized")]
    NotInitialized,
    #[error("secret store is already initialized")]
    AlreadyInitialized,
    #[error("secret store is locked")]
    Locked,
    #[error("secret record was not found: {0}")]
    NotFound(String),
    #[error("secret store unlock failed")]
    UnlockFailed,
    #[error("secret command idempotency result expired")]
    IdempotencyKeyExpired,
    #[error("secret store data is corrupt")]
    CorruptStore,
    #[error("secret store format is unsupported")]
    UnsupportedFormat,
    #[error("invalid secret command: {0}")]
    InvalidArgument(String),
    #[error("secret session is no longer active")]
    SessionExpired,
    #[error("secret store cryptographic operation failed")]
    Crypto,
    #[error("secret store unlock attempts are rate limited")]
    RateLimited,
}

impl SecretStoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotInitialized => "secret_store_not_initialized",
            Self::AlreadyInitialized => "secret_store_already_initialized",
            Self::Locked => "secret_store_locked",
            Self::NotFound(_) => "secret_not_found",
            Self::UnlockFailed => "secret_store_unlock_failed",
            Self::IdempotencyKeyExpired => "idempotency_key_expired",
            Self::CorruptStore => "corrupt_secret_store",
            Self::UnsupportedFormat => "unsupported_secret_store_format",
            Self::InvalidArgument(_) => "invalid_secret_command",
            Self::SessionExpired => "secret_session_expired",
            Self::Crypto => "secret_crypto_failed",
            Self::RateLimited => "secret_unlock_rate_limited",
        }
    }
}
