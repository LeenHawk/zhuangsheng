use std::collections::{BTreeMap, HashSet};

use fancy_regex::{Regex, RegexBuilder};

use super::{RegexMacroMode, TextTransformRule};
use crate::llm::{LlmConfigError, LlmConfigResult};

const BACKTRACK_LIMIT: usize = 250_000;

pub(super) struct CompiledPattern {
    pub regex: Regex,
    pub global: bool,
}

pub(super) fn compile_pattern(
    rule: &TextTransformRule,
    macros: &BTreeMap<String, String>,
) -> LlmConfigResult<CompiledPattern> {
    let source = match rule.macro_mode {
        RegexMacroMode::None => rule.find_regex.clone(),
        RegexMacroMode::Raw => substitute_macros(&rule.find_regex, macros, false),
        RegexMacroMode::Escaped => substitute_macros(&rule.find_regex, macros, true),
    };
    let (mut pattern, flags) = split_pattern(&source)?;
    validate_flags(&flags)?;
    if flags.contains('y') {
        pattern = format!(r"\A(?:{pattern})");
    }
    let mut builder = RegexBuilder::new(&pattern);
    builder
        .case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dot_matches_new_line(flags.contains('s'))
        .unicode_mode(true)
        .backtrack_limit(BACKTRACK_LIMIT);
    let regex = builder
        .build()
        .map_err(|cause| error("invalid_text_transform_pattern", cause.to_string()))?;
    Ok(CompiledPattern {
        regex,
        global: flags.contains('g') && !flags.contains('y'),
    })
}

fn validate_flags(flags: &str) -> LlmConfigResult<()> {
    let mut seen = HashSet::new();
    if flags
        .chars()
        .any(|flag| !matches!(flag, 'd' | 'g' | 'i' | 'm' | 's' | 'u' | 'y') || !seen.insert(flag))
    {
        return Err(error(
            "unsupported_text_transform_flag",
            format!("unsupported or duplicate flags: {flags}"),
        ));
    }
    Ok(())
}

fn split_pattern(source: &str) -> LlmConfigResult<(String, String)> {
    if !source.starts_with('/') {
        return Ok((source.to_owned(), String::new()));
    }
    let bytes = source.as_bytes();
    let closing = (1..bytes.len()).rev().find(|index| {
        bytes[*index] == b'/'
            && bytes[..*index]
                .iter()
                .rev()
                .take_while(|byte| **byte == b'\\')
                .count()
                % 2
                == 0
    });
    let Some(closing) = closing else {
        return Err(error(
            "invalid_text_transform_pattern",
            "slash-delimited regex has no closing slash",
        ));
    };
    Ok((
        source[1..closing].to_owned(),
        source[closing + 1..].to_owned(),
    ))
}

pub(super) fn substitute_macros(
    text: &str,
    macros: &BTreeMap<String, String>,
    escaped: bool,
) -> String {
    let mut output = text.to_owned();
    for (name, value) in macros {
        let value = if escaped {
            fancy_regex::escape(value).into_owned()
        } else {
            value.clone()
        };
        output = output.replace(&format!("{{{{{name}}}}}"), &value);
    }
    output
}

pub(super) fn validation_macros(text: &str) -> BTreeMap<String, String> {
    let Ok(regex) = Regex::new(r"\{\{([A-Za-z_][A-Za-z0-9_]*)\}\}") else {
        return BTreeMap::new();
    };
    regex
        .captures_iter(text)
        .filter_map(|capture| {
            let name = capture.ok()?.get(1)?.as_str().to_owned();
            Some((name, "macro".to_owned()))
        })
        .collect()
}

fn error(code: &'static str, message: impl Into<String>) -> LlmConfigError {
    LlmConfigError::new(code, message)
}
