use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{LlmNodeExecutionSnapshot, ToolApprovalPolicy, ToolGrant};

use super::{
    ResolvedToolDescriptor, ToolRegistryEntrySnapshot, ToolRegistrySnapshot,
    ir::ToolDescriptorIr,
    request_builder::{LlmRequestBuildError, ResolvedRequestTool},
    validate_tool_grant,
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
    if descriptor.tool_id != grant.tool_id
        || descriptor.version != grant.version
        || !valid_name(&descriptor.tool_id)
        || !valid_name(&descriptor.version)
        || !valid_name(&descriptor.name)
        || descriptor
            .description
            .as_ref()
            .is_some_and(|value| value.len() > 4096)
        || pin.descriptor_digest != resolved.descriptor_digest
        || pin.schema_compilation_digests != resolved.schema_compilation_digests
        || pin.implementation_digest != resolved.implementation_digest
        || resolved.implementation_digest.trim().is_empty()
        || resolved.executor_key.trim().is_empty()
    {
        return Err(LlmRequestBuildError::new(
            "tool_descriptor_pin_mismatch",
            format!(
                "tool descriptor does not match its registry pin: {}",
                grant.binding_id
            ),
        ));
    }
    validate_tool_grant(grant, resolved)
        .map_err(|error| LlmRequestBuildError::new(error.code, error.message))?;
    Ok(())
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}
