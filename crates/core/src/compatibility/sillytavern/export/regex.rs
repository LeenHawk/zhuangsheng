use serde_json::{Value, json};

use crate::llm::text_transform::{
    RegexMacroMode, TextTransformPlacement, TextTransformRule, TextTransformScope,
    TextTransformSurface,
};

use super::{export_document, warning};
use crate::compatibility::sillytavern::{
    SillyTavernExportDocument, SillyTavernExportScope, SillyTavernImportWarning,
    SillyTavernPresetKind, SillyTavernResult,
};

pub(super) fn rules_for_scope(
    rules: &[TextTransformRule],
    scope: TextTransformScope,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> Vec<Value> {
    rules
        .iter()
        .filter(|rule| rule.scope == scope)
        .map(|rule| export_rule(rule, warnings))
        .collect()
}

pub(super) fn append_scoped_documents(
    documents: &mut Vec<SillyTavernExportDocument>,
    rules: &[TextTransformRule],
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> SillyTavernResult<()> {
    let global = rules_for_scope(rules, TextTransformScope::Global, warnings);
    if !global.is_empty() {
        documents.push(export_document(
            "sillytavern-global-regex.json",
            SillyTavernPresetKind::RegexScripts,
            SillyTavernExportScope::Global,
            Value::Array(global),
        )?);
    }
    let character = rules_for_scope(rules, TextTransformScope::Character, warnings);
    if !character.is_empty() {
        documents.push(export_document(
            "sillytavern-character-regex.json",
            SillyTavernPresetKind::RegexScripts,
            SillyTavernExportScope::Character,
            json!({"data":{"extensions":{"regex_scripts":character}}}),
        )?);
    }
    Ok(())
}

fn export_rule(rule: &TextTransformRule, warnings: &mut Vec<SillyTavernImportWarning>) -> Value {
    if rule.surfaces.contains(&TextTransformSurface::Canonical) && rule.surfaces.len() > 1 {
        warning(
            warnings,
            "sillytavern_export_surface",
            "canonical combined with ephemeral surfaces cannot round-trip exactly",
            Some(rule.id.clone()),
        );
    }
    json!({
        "id":rule.id,
        "scriptName":rule.name,
        "findRegex":rule.find_regex,
        "replaceString":rule.replace_string,
        "trimStrings":rule.trim_strings,
        "placement":rule.placements.iter().map(placement).collect::<Vec<_>>(),
        "disabled":rule.disabled,
        "markdownOnly":rule.surfaces.contains(&TextTransformSurface::Display),
        "promptOnly":rule.surfaces.contains(&TextTransformSurface::Prompt),
        "runOnEdit":rule.run_on_edit,
        "substituteRegex":match rule.macro_mode { RegexMacroMode::None => 0, RegexMacroMode::Raw => 1, RegexMacroMode::Escaped => 2 },
        "minDepth":rule.min_depth,
        "maxDepth":rule.max_depth
    })
}

fn placement(value: &TextTransformPlacement) -> u8 {
    match value {
        TextTransformPlacement::UserInput => 1,
        TextTransformPlacement::AiOutput => 2,
        TextTransformPlacement::SlashCommand => 3,
        TextTransformPlacement::WorldInfo => 5,
        TextTransformPlacement::Reasoning => 6,
    }
}
