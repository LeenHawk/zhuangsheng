mod generation;
mod prompt;
mod regex;

use serde_json::{Map, Value, json};

use crate::{
    canonical,
    graph::{GenerationOptionsIr, ProviderExtensionsIr},
    llm::context::ContextAssemblySpec,
};

use super::{
    SILLYTAVERN_COMPATIBILITY_VERSION, SillyTavernCompatibilityError, SillyTavernExportBundle,
    SillyTavernExportDocument, SillyTavernExportScope, SillyTavernImportWarning,
    SillyTavernPresetKind, SillyTavernResult,
};

pub fn export_sillytavern_bundle(
    name: &str,
    spec: &ContextAssemblySpec,
    generation: Option<&GenerationOptionsIr>,
    extensions: Option<&ProviderExtensionsIr>,
) -> SillyTavernResult<SillyTavernExportBundle> {
    let mut warnings = Vec::new();
    let mut document = Map::new();
    document.insert("name".into(), Value::String(name.into()));
    generation::export_generation(&mut document, generation, extensions, &mut warnings);
    let (prompts, order, assistant_prefill) = prompt::export_prompts(spec, &mut warnings);
    document.insert("prompts".into(), Value::Array(prompts));
    document.insert(
        "prompt_order".into(),
        json!([{"character_id":100001,"order":order}]),
    );
    if let Some(prefill) = assistant_prefill {
        document.insert("assistant_prefill".into(), Value::String(prefill));
    }
    let mut documents = Vec::new();
    let preset_rules = regex::rules_for_scope(
        &spec.text_transforms,
        crate::llm::text_transform::TextTransformScope::Preset,
        &mut warnings,
    );
    if !preset_rules.is_empty() {
        document.insert("extensions".into(), json!({"regex_scripts":preset_rules}));
    }
    documents.push(export_document(
        "sillytavern-preset.json",
        SillyTavernPresetKind::OpenAi,
        SillyTavernExportScope::Preset,
        Value::Object(document),
    )?);
    regex::append_scoped_documents(&mut documents, &spec.text_transforms, &mut warnings)?;
    Ok(SillyTavernExportBundle {
        compatibility_version: SILLYTAVERN_COMPATIBILITY_VERSION,
        documents,
        warnings,
    })
}

pub(super) fn export_document(
    file_name: &str,
    kind: SillyTavernPresetKind,
    scope: SillyTavernExportScope,
    document: Value,
) -> SillyTavernResult<SillyTavernExportDocument> {
    let source_hash = canonical::hash(&document).map_err(|error| {
        SillyTavernCompatibilityError::new("invalid_sillytavern_export", error.to_string())
    })?;
    Ok(SillyTavernExportDocument {
        file_name: file_name.into(),
        kind,
        scope,
        source_hash,
        document,
    })
}

pub(super) fn warning(
    warnings: &mut Vec<SillyTavernImportWarning>,
    code: &str,
    message: &str,
    field: Option<String>,
) {
    warnings.push(SillyTavernImportWarning {
        code: code.into(),
        message: message.into(),
        field,
    });
}
