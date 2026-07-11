use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub content_hash: String,
    pub byte_size: u64,
    pub media_type: String,
}

impl ArtifactRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.artifact_id.is_empty() || self.artifact_id.len() > 128 {
            return Err("artifact id is invalid");
        }
        if self.byte_size == 0 || self.byte_size > 1024 * 1024 * 1024 {
            return Err("artifact byte size is invalid");
        }
        if self.content_hash.len() != 71
            || !self.content_hash.starts_with("sha256:")
            || !self.content_hash[7..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err("artifact content hash is invalid");
        }
        if self.media_type.is_empty()
            || self.media_type.len() > 128
            || !self.media_type.is_ascii()
            || !self.media_type.contains('/')
        {
            return Err("artifact media type is invalid");
        }
        Ok(())
    }
}
