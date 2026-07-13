mod generation;
mod openai;
mod regex;
mod sections;
mod support;

use crate::{
    canonical,
    llm::{
        context::{ContextNormalizationPolicy, normalize_context_spec},
        text_transform::normalize_text_transforms,
    },
};

use super::{
    SILLYTAVERN_COMPATIBILITY_VERSION, SillyTavernImportInput, SillyTavernImportPreview,
    SillyTavernPresetKind, SillyTavernResult, detect_preset_kind,
};
use support::{
    ImportParts, compatibility_error, empty_context_spec, import_name, merge_text_transforms,
};

const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

pub fn preview_import(
    input: SillyTavernImportInput,
) -> SillyTavernResult<SillyTavernImportPreview> {
    let source = canonical::to_vec(&input.document)
        .map_err(|error| compatibility_error("invalid_sillytavern_json", error.to_string()))?;
    if source.len() > MAX_SOURCE_BYTES {
        return Err(compatibility_error(
            "sillytavern_source_limit",
            "SillyTavern document exceeds sixteen MiB",
        ));
    }
    let kind = detect_preset_kind(&input.document);
    if kind == SillyTavernPresetKind::Unknown {
        return Err(compatibility_error(
            "unknown_sillytavern_preset",
            "document is not a recognized SillyTavern preset or regex export",
        ));
    }
    let name = import_name(&input.document, input.source_name.as_deref());
    let mut parts = ImportParts::new(input.base_spec);
    match kind {
        SillyTavernPresetKind::OpenAi => openai::import_openai(&input.document, &name, &mut parts)?,
        SillyTavernPresetKind::RegexScripts => {
            let rules = regex::parse_top_level(&input.document, &mut parts.warnings)?;
            merge_text_transforms(&mut parts.text_transforms, rules);
        }
        SillyTavernPresetKind::Master => {
            sections::import_master(&input.document, &name, &mut parts)?
        }
        _ => sections::import_section(kind, &input.document, &name, &mut parts)?,
    }
    if kind != SillyTavernPresetKind::RegexScripts {
        let rules = regex::parse_embedded(&input.document, &mut parts.warnings)?;
        merge_text_transforms(&mut parts.text_transforms, rules);
    }
    parts.text_transforms = normalize_text_transforms(parts.text_transforms)
        .map_err(|error| compatibility_error(error.code, error.message))?;
    if parts.context_spec.is_none() && !parts.text_transforms.is_empty() {
        parts.context_spec = Some(empty_context_spec(&name));
    }
    if let Some(spec) = &mut parts.context_spec {
        support::populate_role_macros(spec);
        spec.text_transforms = parts.text_transforms.clone();
        *spec = normalize_context_spec(spec.clone(), &ContextNormalizationPolicy::default())
            .map_err(|error| compatibility_error(error.code, error.message))?;
    }
    parts.inactive_fields.sort();
    parts.inactive_fields.dedup();
    Ok(SillyTavernImportPreview {
        compatibility_version: SILLYTAVERN_COMPATIBILITY_VERSION,
        kind,
        name,
        source_hash: canonical::hash(&input.document)
            .map_err(|error| compatibility_error("invalid_sillytavern_json", error.to_string()))?,
        context_spec: parts.context_spec,
        generation: parts.generation,
        provider_extensions: parts.provider_extensions,
        text_transforms: parts.text_transforms,
        inactive_fields: parts.inactive_fields,
        warnings: parts.warnings,
    })
}
