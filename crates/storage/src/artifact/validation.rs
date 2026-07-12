use zhuangsheng_core::{
    application::artifact::CreateArtifactStagingCommand, artifact::ArtifactRetention,
};

use crate::{StorageError, StorageResult};

pub(super) const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn validate_create(
    command: &CreateArtifactStagingCommand,
    now: i64,
) -> StorageResult<()> {
    command
        .metadata_draft
        .validate(now)
        .map_err(|message| StorageError::InvalidArgument(message.into()))?;
    for id in [
        command.context_id.as_deref(),
        command.node_attempt_id.as_deref(),
        command.tool_call_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if id.is_empty() || id.len() > 128 {
            return Err(StorageError::InvalidArgument(
                "artifact staging owner id is invalid".into(),
            ));
        }
    }
    if command.tool_call_id.is_some() && command.node_attempt_id.is_none() {
        return Err(StorageError::InvalidArgument(
            "tool artifact staging requires a node attempt".into(),
        ));
    }
    match command.metadata_draft.retention {
        ArtifactRetention::Run if command.node_attempt_id.is_none() => Err(
            StorageError::InvalidArgument("run retention requires a node attempt".into()),
        ),
        ArtifactRetention::Context
            if command.context_id.is_none() && command.node_attempt_id.is_none() =>
        {
            Err(StorageError::InvalidArgument(
                "context retention requires a context owner".into(),
            ))
        }
        _ => validate_declared_media(command.declared_media_type.as_deref()),
    }
}

fn validate_declared_media(value: Option<&str>) -> StorageResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let valid = value.len() <= 128
        && value.is_ascii()
        && value == value.to_ascii_lowercase()
        && !value.contains([';', ' '])
        && value
            .split_once('/')
            .is_some_and(|(kind, subtype)| !kind.is_empty() && !subtype.is_empty());
    if valid {
        Ok(())
    } else {
        Err(StorageError::InvalidArgument(
            "declared artifact media type is invalid".into(),
        ))
    }
}

pub(super) fn validate_bytes(bytes: &[u8], declared: Option<&str>) -> Result<&'static str, ()> {
    if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(());
    }
    let detected = detect_media_type(bytes).ok_or(())?;
    if declared.is_some_and(|value| value != detected && value != "application/octet-stream") {
        return Err(());
    }
    Ok(detected)
}

fn detect_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else {
        let text = std::str::from_utf8(bytes).ok()?;
        let trimmed = text.trim_start().as_bytes();
        if starts_ascii_case_insensitive(trimmed, b"<!doctype html")
            || starts_ascii_case_insensitive(trimmed, b"<html")
        {
            Some("text/html")
        } else if starts_ascii_case_insensitive(trimmed, b"<svg") {
            Some("image/svg+xml")
        } else if !text.chars().any(|character| {
            character == '\0' || (character.is_control() && !"\n\r\t".contains(character))
        }) {
            Some("text/plain")
        } else {
            None
        }
    }
}

fn starts_ascii_case_insensitive(value: &[u8], prefix: &[u8]) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}
