use serde_json::Value;

use crate::llm::text_transform::RegexMacroMode;

use super::super::{
    super::SillyTavernImportWarning,
    support::{compatibility_error, warning},
};
use crate::compatibility::sillytavern::SillyTavernResult;

pub(super) fn string(value: Option<&Value>, field: &str) -> SillyTavernResult<String> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            compatibility_error(
                "invalid_sillytavern_regex_rule",
                format!("{field} must be a string"),
            )
        })
}

pub(super) fn boolean(value: Option<&Value>, default: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(default)
}

pub(super) fn signed(value: Option<&Value>) -> Option<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

pub(super) fn unsigned(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| u32::try_from(value).ok())
}

pub(super) fn number_array(
    value: Option<&Value>,
    field: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> Vec<i64> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(values) = value.as_array() else {
        warning(
            warnings,
            "invalid_sillytavern_regex_field",
            "placement must be an array",
            Some(field.to_owned()),
        );
        return Vec::new();
    };
    values.iter().filter_map(Value::as_i64).collect()
}

pub(super) fn strings(
    value: Option<&Value>,
    field: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(values) = value.as_array() else {
        warning(
            warnings,
            "invalid_sillytavern_regex_field",
            "trimStrings must be an array",
            Some(field.to_owned()),
        );
        return Vec::new();
    };
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(super) fn macro_mode(
    value: Option<&Value>,
    field: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> RegexMacroMode {
    match value.and_then(Value::as_i64).unwrap_or(0) {
        0 => RegexMacroMode::None,
        1 => RegexMacroMode::Raw,
        2 => RegexMacroMode::Escaped,
        other => {
            warning(
                warnings,
                "unknown_sillytavern_regex_substitution",
                format!("unknown substituteRegex value {other}; raw regex is preserved"),
                Some(field.to_owned()),
            );
            RegexMacroMode::None
        }
    }
}
