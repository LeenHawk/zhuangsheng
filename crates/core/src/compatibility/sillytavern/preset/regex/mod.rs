mod fields;
mod migration;

use serde_json::Value;

use crate::{
    canonical,
    llm::text_transform::{TextTransformRule, TextTransformScope},
};

use super::{
    super::{SillyTavernImportWarning, SillyTavernResult},
    support::compatibility_error,
};
use fields::{boolean, macro_mode, signed, string, strings, unsigned};
use migration::placement_and_surfaces;

pub(super) fn parse_top_level(
    document: &Value,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> SillyTavernResult<Vec<TextTransformRule>> {
    parse_array(
        document,
        TextTransformScope::Global,
        "regex_scripts",
        warnings,
    )
}

pub(super) fn parse_embedded(
    document: &Value,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> SillyTavernResult<Vec<TextTransformRule>> {
    let candidates = [
        (
            document.pointer("/data/extensions/regex_scripts"),
            TextTransformScope::Character,
            "data.extensions.regex_scripts",
        ),
        (
            document.pointer("/extensions/regex_scripts"),
            TextTransformScope::Preset,
            "extensions.regex_scripts",
        ),
    ];
    let mut rules = Vec::new();
    for (value, scope, path) in candidates {
        if let Some(value) = value {
            rules.extend(parse_array(value, scope, path, warnings)?);
        }
    }
    Ok(rules)
}

fn parse_array(
    value: &Value,
    scope: TextTransformScope,
    path: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> SillyTavernResult<Vec<TextTransformRule>> {
    let values = value.as_array().ok_or_else(|| {
        compatibility_error(
            "invalid_sillytavern_regex_scripts",
            format!("{path} must be an array"),
        )
    })?;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| parse_rule(value, scope, index, path, warnings))
        .collect()
}

fn parse_rule(
    value: &Value,
    scope: TextTransformScope,
    index: usize,
    path: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> SillyTavernResult<TextTransformRule> {
    let object = value.as_object().ok_or_else(|| {
        compatibility_error(
            "invalid_sillytavern_regex_rule",
            format!("{path}[{index}] must be an object"),
        )
    })?;
    let field = |name: &str| format!("{path}[{index}].{name}");
    let (placements, surfaces) = placement_and_surfaces(object, index, path, warnings);
    Ok(TextTransformRule {
        id: rule_id(object.get("id"), value, scope, index)?,
        name: object
            .get("scriptName")
            .and_then(Value::as_str)
            .unwrap_or("Unnamed regex")
            .to_owned(),
        scope,
        order: u32::try_from(index).unwrap_or(u32::MAX),
        find_regex: string(object.get("findRegex"), &field("findRegex"))?,
        replace_string: string(object.get("replaceString"), &field("replaceString"))?,
        trim_strings: strings(object.get("trimStrings"), &field("trimStrings"), warnings),
        placements,
        surfaces,
        disabled: boolean(object.get("disabled"), false),
        run_on_edit: boolean(object.get("runOnEdit"), false),
        macro_mode: macro_mode(
            object.get("substituteRegex"),
            &field("substituteRegex"),
            warnings,
        ),
        min_depth: signed(object.get("minDepth")),
        max_depth: unsigned(object.get("maxDepth")),
    })
}

fn rule_id(
    value: Option<&Value>,
    rule: &Value,
    scope: TextTransformScope,
    index: usize,
) -> SillyTavernResult<String> {
    if let Some(id) = value
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
    {
        return Ok(id.to_owned());
    }
    let hash = canonical::hash(rule).map_err(|error| {
        compatibility_error("invalid_sillytavern_regex_rule", error.to_string())
    })?;
    Ok(format!("st-{}-{index}-{}", scope_name(scope), &hash[7..19]))
}

fn scope_name(scope: TextTransformScope) -> &'static str {
    match scope {
        TextTransformScope::Global => "global",
        TextTransformScope::Character => "character",
        TextTransformScope::Preset => "preset",
    }
}
