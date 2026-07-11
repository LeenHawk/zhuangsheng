use std::{collections::HashSet, fmt};

use super::LongTermMemoryContentV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryValidationError {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for MemoryValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for MemoryValidationError {}

pub fn normalize_content(
    mut content: LongTermMemoryContentV1,
) -> Result<LongTermMemoryContentV1, MemoryValidationError> {
    if content.schema_version != 1 {
        return Err(invalid("memory content schemaVersion must be 1"));
    }
    if content.text.is_empty() || content.text.len() > 64 * 1024 {
        return Err(invalid("memory text must contain 1..=65536 bytes"));
    }
    if content.tags.len() > 64
        || content
            .tags
            .iter()
            .any(|tag| tag.is_empty() || tag.len() > 256)
    {
        return Err(invalid("memory tags exceed count or size limits"));
    }
    content.tags.sort();
    content.tags.dedup();
    let attributes = crate::canonical::to_vec(&content.attributes)
        .map_err(|error| invalid(error.to_string()))?;
    if attributes.len() > 256 * 1024 {
        return Err(invalid("memory attributes exceed 256 KiB"));
    }
    Ok(content)
}

pub fn validate_proposal_material(
    scope_id: &str,
    reason: &str,
    evidence_refs: &[String],
    schema_version: u32,
    policy_version: u32,
) -> Result<(), MemoryValidationError> {
    if scope_id.is_empty() || scope_id.len() > 512 {
        return Err(invalid("memory scope ID is invalid"));
    }
    if reason.is_empty() || reason.len() > 4096 {
        return Err(invalid("proposal reason must contain 1..=4096 bytes"));
    }
    let mut evidence = HashSet::new();
    if evidence_refs.len() > 64
        || evidence_refs
            .iter()
            .any(|value| value.is_empty() || value.len() > 1024 || !evidence.insert(value))
    {
        return Err(invalid("proposal evidence refs are invalid"));
    }
    if schema_version != 1 || policy_version == 0 {
        return Err(invalid("proposal schema/policy version is invalid"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> MemoryValidationError {
    MemoryValidationError {
        code: "invalid_memory_proposal",
        message: message.into(),
    }
}
