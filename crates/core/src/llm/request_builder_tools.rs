use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::{
    graph::{LlmNodeExecutionSnapshot, ToolApprovalPolicy, ToolGrant},
    schema,
};

use super::{
    ResolvedToolDescriptor, ToolRegistryEntrySnapshot, ToolRegistrySnapshot,
    ir::{HostedToolDescriptorIr, MetadataValue, ToolDescriptorIr},
    request_builder::{LlmRequestBuildError, ResolvedRequestTool},
};

pub(super) fn resolve_tools(
    execution: &LlmNodeExecutionSnapshot,
    snapshot: &ToolRegistrySnapshot,
    descriptors: &[ResolvedToolDescriptor],
) -> Result<(Vec<ToolDescriptorIr>, Vec<ResolvedRequestTool>), LlmRequestBuildError> {
    if snapshot.revision.trim().is_empty() {
        return Err(LlmRequestBuildError::new(
            "tool_registry_snapshot_invalid",
            "tool registry snapshot has no revision",
        ));
    }
    let mut pins = BTreeMap::new();
    for pin in &snapshot.entries {
        let key = (pin.tool_id.as_str(), pin.version.as_str());
        if pins.insert(key, pin).is_some() {
            return Err(LlmRequestBuildError::new(
                "tool_registry_snapshot_invalid",
                "tool registry snapshot contains duplicate entries",
            ));
        }
    }
    let mut resolved = BTreeMap::new();
    for descriptor in descriptors {
        let key = (
            descriptor.descriptor.tool_id.as_str(),
            descriptor.descriptor.version.as_str(),
        );
        if resolved.insert(key, descriptor).is_some() {
            return Err(LlmRequestBuildError::new(
                "tool_descriptor_duplicate",
                "resolved tool descriptors contain duplicate identities",
            ));
        }
    }
    let mut names = BTreeSet::new();
    let mut tools = Vec::with_capacity(execution.tools.len());
    let mut request_tools = Vec::with_capacity(execution.tools.len());
    for grant in &execution.tools {
        let descriptor = resolved
            .get(&(grant.tool_id.as_str(), grant.version.as_str()))
            .copied()
            .ok_or_else(|| {
                LlmRequestBuildError::new(
                    "tool_descriptor_missing",
                    format!(
                        "resolved descriptor is missing for binding {}",
                        grant.binding_id
                    ),
                )
            })?;
        let pin = pins
            .get(&(grant.tool_id.as_str(), grant.version.as_str()))
            .copied()
            .ok_or_else(|| {
                LlmRequestBuildError::new(
                    "tool_registry_pin_missing",
                    format!("registry pin is missing for binding {}", grant.binding_id),
                )
            })?;
        validate_descriptor(grant, descriptor, pin)?;
        let exposed_name = grant
            .exposed_name
            .clone()
            .unwrap_or_else(|| descriptor.descriptor.name.clone());
        if !valid_name(&exposed_name) || !names.insert(exposed_name.clone()) {
            return Err(LlmRequestBuildError::new(
                "tool_exposed_name_invalid",
                "tool exposed names must be valid and unique",
            ));
        }
        tools.push(ToolDescriptorIr {
            name: exposed_name.clone(),
            description: descriptor.descriptor.description.clone(),
            input_schema: descriptor.descriptor.input_schema.clone(),
        });
        request_tools.push(ResolvedRequestTool {
            binding_id: grant.binding_id.clone(),
            exposed_name,
            grant: grant.clone(),
            descriptor: descriptor.clone(),
            requires_approval: descriptor.descriptor.effect.requires_approval
                || grant.approval == Some(ToolApprovalPolicy::Always),
        });
    }
    Ok((tools, request_tools))
}

fn validate_descriptor(
    grant: &ToolGrant,
    resolved: &ResolvedToolDescriptor,
    pin: &ToolRegistryEntrySnapshot,
) -> Result<(), LlmRequestBuildError> {
    let descriptor = &resolved.descriptor;
    let computed = descriptor
        .digest()
        .map_err(|error| LlmRequestBuildError::new("tool_descriptor_invalid", error.to_string()))?;
    if descriptor.tool_id != grant.tool_id
        || descriptor.version != grant.version
        || !valid_name(&descriptor.tool_id)
        || !valid_name(&descriptor.version)
        || !valid_name(&descriptor.name)
        || descriptor
            .description
            .as_ref()
            .is_some_and(|value| value.len() > 4096)
        || descriptor.effect.operation_key.trim().is_empty()
        || descriptor.limits.timeout_ms == 0
        || descriptor.limits.max_input_bytes == 0
        || descriptor.limits.max_llm_result_bytes == 0
        || descriptor.limits.max_artifact_bytes == 0
        || computed != resolved.descriptor_digest
        || pin.descriptor_digest != resolved.descriptor_digest
        || pin.schema_compilation_digests != resolved.schema_compilation_digests
        || pin.implementation_digest != resolved.implementation_digest
        || resolved.implementation_digest.trim().is_empty()
    {
        return Err(LlmRequestBuildError::new(
            "tool_descriptor_pin_mismatch",
            format!(
                "tool descriptor does not match its registry pin: {}",
                grant.binding_id
            ),
        ));
    }
    schema::compile(&descriptor.input_schema).map_err(|error| {
        LlmRequestBuildError::new("tool_input_schema_invalid", error.to_string())
    })?;
    let constraints = Value::Object(grant.constraints.clone().into_iter().collect());
    match &descriptor.binding_config_schema {
        Some(spec) => schema::validate(spec, &constraints).map_err(|error| {
            LlmRequestBuildError::new("tool_binding_config_invalid", error.to_string())
        })?,
        None if !grant.constraints.is_empty() => {
            return Err(LlmRequestBuildError::new(
                "tool_binding_config_unsupported",
                "tool grant has constraints but its descriptor has no binding config schema",
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
        return Err(LlmRequestBuildError::new(
            "tool_required_scope_missing",
            format!(
                "tool grant is missing a descriptor-required scope: {}",
                grant.binding_id
            ),
        ));
    }
    Ok(())
}

pub(super) fn resolve_hosted_tools(
    execution: &LlmNodeExecutionSnapshot,
    approved: &BTreeSet<String>,
) -> Result<Vec<HostedToolDescriptorIr>, LlmRequestBuildError> {
    let mut output = Vec::with_capacity(execution.hosted_tools.len());
    for binding in &execution.hosted_tools {
        if binding.operation_key != execution.operation.operation_key {
            return Err(LlmRequestBuildError::new(
                "hosted_tool_operation_mismatch",
                "hosted tool operation does not match the pinned generation operation",
            ));
        }
        if binding.effect.requires_approval && !approved.contains(&binding.binding_id) {
            return Err(LlmRequestBuildError::new(
                "hosted_tool_approval_required",
                format!(
                    "hosted tool requires approval before exposure: {}",
                    binding.binding_id
                ),
            ));
        }
        let mut config = BTreeMap::new();
        for (key, value) in &binding.model_facing_config {
            if sensitive_name(key) {
                return Err(LlmRequestBuildError::new(
                    "hosted_tool_config_unsafe",
                    "hosted tool config contains a sensitive field name",
                ));
            }
            config.insert(key.clone(), metadata_value(value)?);
        }
        output.push(HostedToolDescriptorIr {
            binding_id: binding.binding_id.clone(),
            hosted_kind: binding.hosted_kind.clone(),
            config,
        });
    }
    Ok(output)
}

fn metadata_value(value: &Value) -> Result<MetadataValue, LlmRequestBuildError> {
    match value {
        Value::Null => Ok(MetadataValue::Null),
        Value::Bool(value) => Ok(MetadataValue::Boolean(*value)),
        Value::Number(value) => Ok(MetadataValue::Number(value.clone())),
        Value::String(value) if value.len() <= 4096 => Ok(MetadataValue::String(value.clone())),
        Value::String(_) | Value::Array(_) | Value::Object(_) => Err(LlmRequestBuildError::new(
            "hosted_tool_config_invalid",
            "hosted tool model-facing config must contain bounded scalar values",
        )),
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn sensitive_name(value: &str) -> bool {
    let name = value.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "host"
    ) || ["token", "secret", "credential", "signature"]
        .iter()
        .any(|needle| name.contains(needle))
}
