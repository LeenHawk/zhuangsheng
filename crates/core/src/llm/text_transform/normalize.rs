use std::collections::HashSet;

use super::{TextTransformRule, pattern::compile_pattern};
use crate::llm::{LlmConfigError, LlmConfigResult};

const MAX_RULES: usize = 256;
const MAX_PATTERN_BYTES: usize = 64 * 1024;
const MAX_REPLACEMENT_BYTES: usize = 64 * 1024;

pub fn normalize_text_transforms(
    mut rules: Vec<TextTransformRule>,
) -> LlmConfigResult<Vec<TextTransformRule>> {
    if rules.len() > MAX_RULES {
        return Err(error(
            "text_transform_rule_limit",
            "preset has more than 256 text transform rules",
        ));
    }
    rules.sort_by_key(|rule| (rule.priority, rule.order));
    let mut ids = HashSet::new();
    for (index, rule) in rules.iter_mut().enumerate() {
        rule.id = rule.id.trim().to_owned();
        rule.name = rule.name.trim().to_owned();
        if rule.id.is_empty() || rule.id.len() > 128 || !ids.insert(rule.id.clone()) {
            return Err(error(
                "invalid_text_transform_id",
                format!("invalid or duplicate rule id at index {index}"),
            ));
        }
        validate_rule_limits(rule)?;
        rule.targets.sort_by_key(|value| *value as u8);
        rule.targets.dedup();
        rule.surfaces.sort_by_key(|value| *value as u8);
        rule.surfaces.dedup();
        if rule.surfaces.is_empty() {
            return Err(error(
                "invalid_text_transform_rule",
                format!("rule has no surface: {}", rule.id),
            ));
        }
        validate_depth(rule)?;
        rule.order = u32::try_from(index)
            .map_err(|_| error("text_transform_rule_limit", "rule order overflow"))?;
        compile_pattern(rule, &super::pattern::validation_macros(&rule.find_regex)).map_err(
            |cause| {
                error(
                    "invalid_text_transform_pattern",
                    format!("{}: {}", rule.id, cause.message),
                )
            },
        )?;
    }
    Ok(rules)
}

fn validate_rule_limits(rule: &TextTransformRule) -> LlmConfigResult<()> {
    if rule.name.len() > 200
        || rule.find_regex.is_empty()
        || rule.find_regex.len() > MAX_PATTERN_BYTES
        || rule.replace_string.len() > MAX_REPLACEMENT_BYTES
        || rule.trim_strings.len() > 128
        || rule
            .trim_strings
            .iter()
            .any(|value| value.len() > MAX_REPLACEMENT_BYTES)
    {
        return Err(error(
            "invalid_text_transform_rule",
            format!("rule exceeds limits: {}", rule.id),
        ));
    }
    Ok(())
}

fn validate_depth(rule: &mut TextTransformRule) -> LlmConfigResult<()> {
    if rule.min_depth.is_some_and(|value| value < -1)
        || rule.max_depth.is_some_and(|value| value > 10_000)
        || matches!((rule.min_depth, rule.max_depth), (Some(min), Some(max)) if min >= 0 && min as u32 > max)
    {
        return Err(error(
            "invalid_text_transform_depth",
            format!("invalid depth range: {}", rule.id),
        ));
    }
    if rule.min_depth == Some(-1) {
        rule.min_depth = None;
    }
    Ok(())
}

fn error(code: &'static str, message: impl Into<String>) -> LlmConfigError {
    LlmConfigError::new(code, message)
}
