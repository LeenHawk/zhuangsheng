use std::collections::{BTreeMap, BTreeSet};

use super::types::{TemplateProgramV1, TemplateSegment, TemplateVariableSource};
use crate::llm::{LlmConfigError, LlmConfigResult};

pub(super) fn compile_template(
    template: &str,
    variables: &BTreeMap<String, TemplateVariableSource>,
) -> LlmConfigResult<TemplateProgramV1> {
    if template.len() > 1024 * 1024 {
        return Err(LlmConfigError::new(
            "context_template_limit",
            "template exceeds the one MiB limit",
        ));
    }
    let mut segments = Vec::new();
    let mut names = BTreeSet::new();
    let mut text = String::new();
    let mut index = 0;
    while index < template.len() {
        let remaining = &template[index..];
        if remaining.starts_with("\\{{") {
            text.push_str("{{");
            index += 3;
            continue;
        }
        if remaining.starts_with("{{") {
            push_text(&mut segments, &mut text);
            let after_open = index + 2;
            let close = template[after_open..].find("}}").ok_or_else(|| {
                LlmConfigError::new("invalid_context_template", "unclosed template placeholder")
            })? + after_open;
            let name = &template[after_open..close];
            if !valid_identifier(name) {
                return Err(LlmConfigError::new(
                    "invalid_context_template_variable",
                    format!("invalid template variable: {name}"),
                ));
            }
            names.insert(name.to_owned());
            segments.push(TemplateSegment::Variable { name: name.into() });
            index = close + 2;
            continue;
        }
        let character = remaining.chars().next().expect("non-empty remainder");
        text.push(character);
        index += character.len_utf8();
    }
    push_text(&mut segments, &mut text);
    let declared: BTreeSet<_> = variables.keys().cloned().collect();
    if names != declared {
        return Err(LlmConfigError::new(
            "context_template_variable_mismatch",
            "placeholder names must exactly match declared variable names",
        ));
    }
    Ok(TemplateProgramV1 {
        syntax_version: 1,
        segments,
    })
}

fn push_text(segments: &mut Vec<TemplateSegment>, text: &mut String) {
    if !text.is_empty() {
        segments.push(TemplateSegment::Text {
            value: std::mem::take(text),
        });
    }
}

fn valid_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 {
        return false;
    }
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
