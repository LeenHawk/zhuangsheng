use serde_json::Value;

use crate::{
    canonical,
    llm::ir::{
        ContextProvenanceIr, ContextSensitivity, ContextTrust, LlmContentPartIr, ProvenanceRole,
    },
    selector,
};

use super::{
    ContextAssemblyError, ContextAssemblyInput, ContextAssemblyResult, ContextItem,
    ContextPosition, ContextProvenance, ContextRole, ResolvedContextBinding,
    candidate::ContextCandidate,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn build_candidate(
    item: &ContextItem,
    item_index: usize,
    sub_index: usize,
    history_order: Option<u64>,
    role: ContextRole,
    content: Vec<LlmContentPartIr>,
    provenance: ContextProvenance,
    transformations: Vec<String>,
    content_hash: Option<String>,
    relevance_score_micros: Option<i64>,
) -> ContextAssemblyResult<ContextCandidate> {
    if content.is_empty() {
        return Err(ContextAssemblyError::new(
            "context_content_empty",
            format!("context item has empty content: {}", item.id),
        ));
    }
    let id = format!("ctx:{}:{item_index}:{sub_index}", item.id);
    let provenance_id = format!("ctxprov:{item_index}:{sub_index}");
    Ok(ContextCandidate {
        id,
        sub_index,
        history_order,
        role,
        content_hash: match content_hash {
            Some(hash) => hash,
            None => canonical::hash(&content)?,
        },
        content,
        provenance: ContextProvenanceIr {
            id: provenance_id,
            item_id: item.id.clone(),
            source_type: provenance.source_type,
            source_id: provenance.source_id,
            trust: provenance.trust,
            sensitivity: provenance.sensitivity,
            final_role: provenance_role(role),
            transformations,
        },
        relevance_score_micros,
        included: false,
        token_count: 0,
    })
}

pub(super) fn binding<'a>(
    input: &'a ContextAssemblyInput,
    binding_id: &str,
) -> ContextAssemblyResult<&'a ResolvedContextBinding> {
    let binding = input.bindings.get(binding_id).ok_or_else(|| {
        ContextAssemblyError::new(
            "context_binding_missing",
            format!("resolved binding is missing: {binding_id}"),
        )
    })?;
    if binding.binding_id != binding_id || binding.version.is_empty() || binding.scope.is_empty() {
        return Err(ContextAssemblyError::new(
            "context_binding_invalid",
            format!("resolved binding identity is invalid: {binding_id}"),
        ));
    }
    Ok(binding)
}

pub(super) fn authorized_data_role(
    requested: ContextRole,
    trust: ContextTrust,
    allowed: &[ContextRole],
    transformations: &mut Vec<String>,
) -> ContextAssemblyResult<ContextRole> {
    let mut role = match trust {
        ContextTrust::RuntimePolicy => {
            if requested != ContextRole::Policy {
                transformations.push("role_normalized_to_policy".into());
            }
            ContextRole::Policy
        }
        ContextTrust::ExternalUntrusted => ContextRole::Context,
        ContextTrust::UserInput if requested == ContextRole::User => ContextRole::User,
        ContextTrust::UserInput => ContextRole::Context,
        ContextTrust::TrustedConfig => trusted_data_role(requested),
    };
    if role != requested
        && !transformations
            .iter()
            .any(|value| value == "role_normalized_to_policy")
    {
        transformations.push("role_downgraded_by_trust".into());
    }
    if !allowed.contains(&role) {
        if allowed.contains(&ContextRole::Context) && role != ContextRole::Policy {
            role = ContextRole::Context;
            transformations.push("role_downgraded_by_binding".into());
        } else {
            return Err(ContextAssemblyError::new(
                "context_role_unauthorized",
                "resolved binding does not authorize the final role",
            ));
        }
    }
    Ok(role)
}

fn trusted_data_role(requested: ContextRole) -> ContextRole {
    match requested {
        ContextRole::Policy => ContextRole::System,
        ContextRole::User | ContextRole::Assistant => ContextRole::Context,
        role => role,
    }
}

pub(super) fn trusted_config_role(
    item: &ContextItem,
    transformations: &mut Vec<String>,
) -> ContextRole {
    match item.requested_role {
        ContextRole::Policy => {
            transformations.push("policy_role_requires_runtime_trust".into());
            ContextRole::System
        }
        ContextRole::Assistant if item.position == ContextPosition::AssistantPrefill => {
            ContextRole::Assistant
        }
        ContextRole::User | ContextRole::Assistant => {
            transformations.push("trusted_config_role_downgraded_to_context".into());
            ContextRole::Context
        }
        role => role,
    }
}

pub(super) fn template_role(
    item: &ContextItem,
    trust: ContextTrust,
    transformations: &mut Vec<String>,
) -> ContextRole {
    match trust {
        ContextTrust::RuntimePolicy => ContextRole::Policy,
        ContextTrust::TrustedConfig => trusted_config_role(item, transformations),
        ContextTrust::UserInput if item.requested_role == ContextRole::User => ContextRole::User,
        ContextTrust::UserInput | ContextTrust::ExternalUntrusted => {
            transformations.push("template_taint_downgrade".into());
            ContextRole::Context
        }
    }
}

pub(super) fn select_value(
    selection: &crate::graph::InputSelector,
    value: &Value,
) -> ContextAssemblyResult<Value> {
    selector::select(selection, value, 1024).map_err(|message| {
        let code = if message.starts_with("JSON Pointer did not match:")
            || message == "JSONPath expected one match but found 0"
        {
            "context_template_value_missing"
        } else {
            "context_template_selection_failed"
        };
        ContextAssemblyError::new(code, message)
    })
}

pub(super) fn value_text(value: &Value) -> ContextAssemblyResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        _ => canonical::to_string(value).map_err(|error| {
            ContextAssemblyError::new("context_value_serialization_failed", error.to_string())
        }),
    }
}

pub(super) fn least_trusted(left: ContextTrust, right: ContextTrust) -> ContextTrust {
    if trust_rank(left) >= trust_rank(right) {
        left
    } else {
        right
    }
}

fn trust_rank(value: ContextTrust) -> u8 {
    match value {
        ContextTrust::RuntimePolicy => 0,
        ContextTrust::TrustedConfig => 1,
        ContextTrust::UserInput => 2,
        ContextTrust::ExternalUntrusted => 3,
    }
}

pub(super) fn most_sensitive(
    left: ContextSensitivity,
    right: ContextSensitivity,
) -> ContextSensitivity {
    if sensitivity_rank(left) >= sensitivity_rank(right) {
        left
    } else {
        right
    }
}

fn sensitivity_rank(value: ContextSensitivity) -> u8 {
    match value {
        ContextSensitivity::Public => 0,
        ContextSensitivity::Private => 1,
        ContextSensitivity::Sensitive => 2,
    }
}

pub(super) fn provenance_role(role: ContextRole) -> ProvenanceRole {
    match role {
        ContextRole::Policy => ProvenanceRole::Policy,
        ContextRole::System => ProvenanceRole::System,
        ContextRole::Developer => ProvenanceRole::Developer,
        ContextRole::Context => ProvenanceRole::Context,
        ContextRole::User => ProvenanceRole::User,
        ContextRole::Assistant => ProvenanceRole::Assistant,
    }
}
