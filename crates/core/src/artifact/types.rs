use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactClassification {
    Public,
    Private,
    Sensitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ArtifactRetention {
    Ephemeral { expires_at: i64 },
    Run,
    Context,
    Pinned,
    AuditUntil { timestamp: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMetadataDraft {
    pub name: Option<String>,
    pub classification: ArtifactClassification,
    pub retention: ArtifactRetention,
}

impl ArtifactMetadataDraft {
    pub fn validate(&self, now: i64) -> Result<(), &'static str> {
        if self.name.as_ref().is_some_and(|name| {
            name.is_empty()
                || name.len() > 255
                || name.chars().any(char::is_control)
                || name.contains(['/', '\\'])
        }) {
            return Err("artifact name is invalid");
        }
        match self.retention {
            ArtifactRetention::Ephemeral { expires_at } if expires_at <= now => {
                Err("artifact expiry must be in the future")
            }
            ArtifactRetention::AuditUntil { timestamp } if timestamp <= now => {
                Err("artifact audit retention must be in the future")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStagingStatus {
    Uploading,
    Staged,
    Validated,
    Quarantined,
    Deleting,
    Deleted,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactStagingView {
    pub staging_id: String,
    pub status: ArtifactStagingStatus,
    pub lifecycle_generation: u64,
    pub byte_size: Option<u64>,
    pub content_hash: Option<String>,
    pub validated_media_type: Option<String>,
}

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
