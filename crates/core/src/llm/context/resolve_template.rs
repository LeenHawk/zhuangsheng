use std::collections::BTreeMap;

use serde_json::Value;

use crate::llm::ir::{ContextSensitivity, ContextTrust, LlmContentPartIr};

use super::{
    ContextAssemblyError, ContextAssemblyInput, ContextAssemblyResult, ContextItem,
    ContextProvenance, TemplateMissingPolicy, TemplateProgramV1, TemplateSegment,
    TemplateVariableSource,
    candidate::ContextCandidate,
    resolve_support::{
        binding, build_candidate, least_trusted, most_sensitive, select_value, template_role,
        value_text,
    },
};

pub(super) fn template_candidate(
    input: &ContextAssemblyInput,
    item: &ContextItem,
    item_index: usize,
    variables: &BTreeMap<String, TemplateVariableSource>,
    on_missing: TemplateMissingPolicy,
    program: &TemplateProgramV1,
) -> ContextAssemblyResult<ContextCandidate> {
    let mut resolved = BTreeMap::new();
    let mut trust = ContextTrust::TrustedConfig;
    let mut sensitivity = ContextSensitivity::Public;
    let mut transformations = Vec::new();
    for (name, source) in variables {
        match resolve_variable(input, source) {
            Ok((value, provenance)) => {
                trust = least_trusted(trust, provenance.trust);
                sensitivity = most_sensitive(sensitivity, provenance.sensitivity);
                resolved.insert(name.clone(), value_text(&value)?);
            }
            Err(error)
                if on_missing == TemplateMissingPolicy::Empty && is_missing_error(&error) =>
            {
                transformations.push(format!("missing_variable_empty:{name}"));
                resolved.insert(name.clone(), String::new());
            }
            Err(error) => return Err(error),
        }
    }
    let mut text = String::new();
    for segment in &program.segments {
        match segment {
            TemplateSegment::Text { value } => text.push_str(value),
            TemplateSegment::Variable { name } => {
                text.push_str(resolved.get(name).ok_or_else(|| {
                    ContextAssemblyError::new(
                        "context_template_variable_missing",
                        format!("compiled template variable is missing: {name}"),
                    )
                })?)
            }
        }
    }
    let role = template_role(item, trust, &mut transformations);
    build_candidate(
        item,
        item_index,
        0,
        None,
        role,
        vec![LlmContentPartIr::Text { text }],
        ContextProvenance {
            source_type: "template".into(),
            source_id: item.id.clone(),
            trust,
            sensitivity,
        },
        transformations,
        None,
        None,
    )
}

fn resolve_variable(
    input: &ContextAssemblyInput,
    source: &TemplateVariableSource,
) -> ContextAssemblyResult<(Value, ContextProvenance)> {
    match source {
        TemplateVariableSource::Literal { value } => Ok((
            value.clone(),
            ContextProvenance {
                source_type: "template_literal".into(),
                source_id: "literal".into(),
                trust: ContextTrust::TrustedConfig,
                sensitivity: ContextSensitivity::Public,
            },
        )),
        TemplateVariableSource::Input {
            selector: selection,
        } => Ok((
            select_value(selection, &input.node_input)?,
            ContextProvenance {
                source_type: "input".into(),
                source_id: "node_input".into(),
                trust: ContextTrust::UserInput,
                sensitivity: ContextSensitivity::Private,
            },
        )),
        TemplateVariableSource::Binding {
            binding_id,
            selector: selection,
        } => {
            let binding = binding(input, binding_id)?;
            let value = binding.template_value.as_ref().ok_or_else(|| {
                ContextAssemblyError::new(
                    "context_template_binding_missing",
                    format!("binding has no template value: {binding_id}"),
                )
            })?;
            let provenance = binding.template_provenance.clone().ok_or_else(|| {
                ContextAssemblyError::new(
                    "context_template_provenance_missing",
                    format!("binding has no template provenance: {binding_id}"),
                )
            })?;
            Ok((select_value(selection, value)?, provenance))
        }
    }
}

fn is_missing_error(error: &ContextAssemblyError) -> bool {
    matches!(
        error.code,
        "context_binding_missing"
            | "context_template_binding_missing"
            | "context_template_value_missing"
    )
}
