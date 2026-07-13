use serde_json::Value;

use crate::{
    graph::{GenerationOptionsIr, ProviderExtensionsIr},
    llm::{
        context::{ContextAssemblyMode, ContextAssemblySpec},
        text_transform::TextTransformRule,
    },
};

use super::super::{
    SillyTavernCompatibilityError, SillyTavernImportWarning, SillyTavernPresetKind,
};

pub(super) struct ImportParts {
    pub context_spec: Option<ContextAssemblySpec>,
    pub generation: Option<GenerationOptionsIr>,
    pub provider_extensions: Option<ProviderExtensionsIr>,
    pub text_transforms: Vec<TextTransformRule>,
    pub inactive_fields: Vec<String>,
    pub warnings: Vec<SillyTavernImportWarning>,
}

impl ImportParts {
    pub fn new(context_spec: Option<ContextAssemblySpec>) -> Self {
        let text_transforms = context_spec
            .as_ref()
            .map(|spec| spec.text_transforms.clone())
            .unwrap_or_default();
        Self {
            context_spec,
            generation: None,
            provider_extensions: None,
            text_transforms,
            inactive_fields: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

pub(super) fn merge_text_transforms(
    target: &mut Vec<TextTransformRule>,
    imported: Vec<TextTransformRule>,
) {
    for rule in imported {
        if let Some(existing) = target.iter_mut().find(|value| value.id == rule.id) {
            *existing = rule;
        } else {
            target.push(rule);
        }
    }
}

pub(super) fn compatibility_error(
    code: &'static str,
    message: impl Into<String>,
) -> SillyTavernCompatibilityError {
    SillyTavernCompatibilityError::new(code, message)
}

pub(super) fn warning(
    warnings: &mut Vec<SillyTavernImportWarning>,
    code: impl Into<String>,
    message: impl Into<String>,
    field: impl Into<Option<String>>,
) {
    warnings.push(SillyTavernImportWarning {
        code: code.into(),
        message: message.into(),
        field: field.into(),
    });
}

pub(super) fn import_name(document: &Value, source_name: Option<&str>) -> String {
    document
        .get("name")
        .and_then(Value::as_str)
        .or(source_name)
        .unwrap_or("SillyTavern import")
        .trim()
        .chars()
        .take(200)
        .collect()
}

pub(super) fn inactive(
    parts: &mut ImportParts,
    field: impl Into<String>,
    message: impl Into<String>,
) {
    let field = field.into();
    parts.inactive_fields.push(field.clone());
    warning(
        &mut parts.warnings,
        "sillytavern_field_inactive",
        message,
        Some(field),
    );
}

pub(super) fn section_kind(key: &str) -> Option<SillyTavernPresetKind> {
    match key {
        "context" => Some(SillyTavernPresetKind::Context),
        "instruct" => Some(SillyTavernPresetKind::Instruct),
        "sysprompt" => Some(SillyTavernPresetKind::SystemPrompt),
        "preset" => Some(SillyTavernPresetKind::TextCompletion),
        "reasoning" => Some(SillyTavernPresetKind::Reasoning),
        _ => None,
    }
}

pub(super) fn empty_context_spec(name: &str) -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: None,
        name: Some(name.into()),
        mode: ContextAssemblyMode::Chat,
        items: Vec::new(),
        budget: None,
        post_process: Vec::new(),
        text_transforms: Vec::new(),
        preview: None,
    }
}
