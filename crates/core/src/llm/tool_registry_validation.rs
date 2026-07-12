use std::collections::BTreeSet;

use serde_json::Value;
use thiserror::Error;

use crate::{
    canonical,
    graph::{ToolGrant, ToolScopeKind},
    schema::{self, SchemaCompilationDraft},
};

use super::{
    ResolvedToolDescriptor, ToolDescriptor, ToolRegistryEntrySnapshot, ToolRegistrySnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct ToolRegistryError {
    pub code: &'static str,
    pub message: String,
}

impl ToolRegistryError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub fn compile_tool_descriptor(
    descriptor: &ToolDescriptor,
) -> Result<Vec<SchemaCompilationDraft>, ToolRegistryError> {
    let valid_limits = descriptor.limits.timeout_ms > 0
        && descriptor.limits.timeout_ms <= 60 * 60 * 1000
        && descriptor.limits.max_input_bytes > 0
        && descriptor.limits.max_input_bytes <= 16 * 1024 * 1024
        && descriptor.limits.max_llm_result_bytes > 0
        && descriptor.limits.max_llm_result_bytes <= 16 * 1024 * 1024
        && descriptor.limits.max_artifact_bytes > 0
        && descriptor.limits.max_artifact_bytes <= 1024 * 1024 * 1024;
    if !valid_name(&descriptor.tool_id)
        || !valid_name(&descriptor.version)
        || !valid_name(&descriptor.name)
        || descriptor
            .description
            .as_ref()
            .is_some_and(|value| value.len() > 4096)
        || descriptor.effect.operation_key.trim().is_empty()
        || descriptor.effect.operation_key.len() > 256
        || !valid_limits
    {
        return Err(ToolRegistryError::new(
            "tool_descriptor_invalid",
            "tool descriptor identity, effect, or limits are invalid",
        ));
    }
    let mut scopes = BTreeSet::new();
    if descriptor.required_scopes.iter().any(|scope| {
        scope.scope.trim().is_empty()
            || scope.scope.len() > 256
            || !scopes.insert((scope_kind_key(scope.kind), scope.scope.as_str()))
    }) {
        return Err(ToolRegistryError::new(
            "tool_descriptor_scope_invalid",
            "tool descriptor required scopes are invalid or duplicated",
        ));
    }
    let mut compilations =
        vec![schema::compile(&descriptor.input_schema).map_err(|error| {
            ToolRegistryError::new("tool_input_schema_invalid", error.to_string())
        })?];
    if let Some(binding) = &descriptor.binding_config_schema {
        compilations.push(schema::compile(binding).map_err(|error| {
            ToolRegistryError::new("tool_binding_schema_invalid", error.to_string())
        })?);
    }
    Ok(compilations)
}

pub fn build_tool_registry_snapshot(
    descriptors: &[ResolvedToolDescriptor],
) -> Result<ToolRegistrySnapshot, ToolRegistryError> {
    let mut entries: Vec<_> = descriptors
        .iter()
        .map(|resolved| ToolRegistryEntrySnapshot {
            tool_id: resolved.descriptor.tool_id.clone(),
            version: resolved.descriptor.version.clone(),
            descriptor_digest: resolved.descriptor_digest.clone(),
            schema_compilation_digests: resolved.schema_compilation_digests.clone(),
            implementation_digest: resolved.implementation_digest.clone(),
        })
        .collect();
    entries.sort_by(|left, right| {
        (&left.tool_id, &left.version).cmp(&(&right.tool_id, &right.version))
    });
    if entries
        .windows(2)
        .any(|pair| pair[0].tool_id == pair[1].tool_id && pair[0].version == pair[1].version)
    {
        return Err(ToolRegistryError::new(
            "tool_registry_duplicate",
            "tool registry snapshot contains duplicate identities",
        ));
    }
    let digest = canonical::hash(&entries).map_err(|error| {
        ToolRegistryError::new("tool_registry_digest_failed", error.to_string())
    })?;
    Ok(ToolRegistrySnapshot {
        revision: format!("tool-registry-v1:{digest}"),
        entries,
    })
}

pub fn validate_resolved_tool_descriptor(
    resolved: &ResolvedToolDescriptor,
) -> Result<(), ToolRegistryError> {
    let compilation_digests: Vec<_> = compile_tool_descriptor(&resolved.descriptor)?
        .into_iter()
        .map(|item| item.compiled_payload_hash)
        .collect();
    let descriptor_digest = resolved
        .descriptor
        .digest()
        .map_err(|error| ToolRegistryError::new("tool_descriptor_invalid", error.to_string()))?;
    if resolved.descriptor_digest != descriptor_digest
        || resolved.schema_compilation_digests != compilation_digests
        || resolved.implementation_digest.is_empty()
        || resolved.implementation_digest.len() > 256
        || !valid_name(&resolved.executor_key)
    {
        return Err(ToolRegistryError::new(
            "tool_descriptor_pin_mismatch",
            "resolved tool descriptor does not match its schema or implementation pins",
        ));
    }
    Ok(())
}

pub fn validate_tool_grant(
    grant: &ToolGrant,
    resolved: &ResolvedToolDescriptor,
) -> Result<(), ToolRegistryError> {
    validate_resolved_tool_descriptor(resolved)?;
    let descriptor = &resolved.descriptor;
    if descriptor.tool_id != grant.tool_id || descriptor.version != grant.version {
        return Err(ToolRegistryError::new(
            "tool_grant_identity_mismatch",
            "tool grant does not match its resolved descriptor",
        ));
    }
    let constraints = Value::Object(grant.constraints.clone().into_iter().collect());
    match &descriptor.binding_config_schema {
        Some(spec) => schema::validate(spec, &constraints).map_err(|error| {
            ToolRegistryError::new("tool_binding_config_invalid", error.to_string())
        })?,
        None if !grant.constraints.is_empty() => {
            return Err(ToolRegistryError::new(
                "tool_binding_config_unsupported",
                "tool grant has constraints but its descriptor has no binding schema",
            ));
        }
        None => {}
    }
    if descriptor.required_scopes.iter().any(|required| {
        !grant
            .scopes
            .iter()
            .any(|actual| actual.kind == required.kind && actual.scope == required.scope)
    }) {
        return Err(ToolRegistryError::new(
            "tool_required_scope_missing",
            "tool grant is missing a descriptor-required scope",
        ));
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn scope_kind_key(kind: ToolScopeKind) -> u8 {
    match kind {
        ToolScopeKind::MemoryRead => 0,
        ToolScopeKind::MemoryProposal => 1,
        ToolScopeKind::StatePatch => 2,
        ToolScopeKind::ArtifactRead => 3,
        ToolScopeKind::ArtifactWrite => 4,
        ToolScopeKind::Network => 5,
        ToolScopeKind::LocalNetwork => 6,
    }
}
