use serde::{Deserialize, Serialize};

use super::{LlmConfigError, LlmConfigResult};

const MAX_SECRET_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretScheme {
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub scheme: SecretScheme,
    pub id: String,
}

impl SecretRef {
    pub fn validate(&self) -> LlmConfigResult<()> {
        if self.id.is_empty() || self.id.len() > MAX_SECRET_ID_BYTES {
            return Err(LlmConfigError::new(
                "invalid_secret_ref",
                "secret id must contain 1..=128 bytes",
            ));
        }
        if !self
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(LlmConfigError::new(
                "invalid_secret_ref",
                "secret id contains unsupported characters",
            ));
        }
        Ok(())
    }
}
