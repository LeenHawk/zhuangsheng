use fancy_regex::Captures;

use super::pattern::{compile_pattern, substitute_macros};
use super::{TextTransformContext, TextTransformOutput, TextTransformRule};
use crate::llm::{LlmConfigError, LlmConfigResult};

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

pub fn apply_text_transforms(
    input: &str,
    rules: &[TextTransformRule],
    context: &TextTransformContext,
) -> LlmConfigResult<TextTransformOutput> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(error(
            "text_transform_input_limit",
            "text transform input exceeds four MiB",
        ));
    }
    let mut text = input.to_owned();
    let mut applied_rule_ids = Vec::new();
    for rule in rules.iter().filter(|rule| applies(rule, context)) {
        let next = apply_rule(&text, rule, context)?;
        if next != text {
            applied_rule_ids.push(rule.id.clone());
        }
        text = next;
        if text.len() > MAX_OUTPUT_BYTES {
            return Err(error(
                "text_transform_output_limit",
                format!("output exceeds eight MiB after {}", rule.id),
            ));
        }
    }
    Ok(TextTransformOutput {
        text,
        applied_rule_ids,
    })
}

fn applies(rule: &TextTransformRule, context: &TextTransformContext) -> bool {
    if rule.disabled || (context.is_edit && !rule.run_on_edit) {
        return false;
    }
    if context
        .target
        .is_some_and(|value| !rule.targets.contains(&value))
        || context
            .surface
            .is_some_and(|value| !rule.surfaces.contains(&value))
    {
        return false;
    }
    context.depth.is_none_or(|depth| {
        rule.min_depth
            .is_none_or(|min| min < 0 || depth >= min as u32)
            && rule.max_depth.is_none_or(|max| depth <= max)
    })
}

fn apply_rule(
    input: &str,
    rule: &TextTransformRule,
    context: &TextTransformContext,
) -> LlmConfigResult<String> {
    let compiled = compile_pattern(rule, &context.macros)?;
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    let mut matched_any = false;
    for capture in compiled.regex.captures_iter(input) {
        let capture = capture
            .map_err(|cause| error("text_transform_runtime", format!("{}: {cause}", rule.id)))?;
        let matched = capture.get(0).expect("capture zero");
        output.push_str(&input[cursor..matched.start()]);
        output.push_str(&replacement(input, rule, &capture, context));
        cursor = matched.end();
        matched_any = true;
        if !compiled.global {
            break;
        }
    }
    if !matched_any {
        return Ok(input.to_owned());
    }
    output.push_str(&input[cursor..]);
    Ok(output)
}

fn replacement(
    input: &str,
    rule: &TextTransformRule,
    captures: &Captures<'_>,
    context: &TextTransformContext,
) -> String {
    let source = rule.replace_string.clone();
    let matched = captures.get(0).expect("capture zero");
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;
    while index < source.len() {
        if source.as_bytes()[index] == b'$'
            && let Some((end, value)) = capture_reference(
                &source,
                index,
                input,
                matched.start(),
                matched.end(),
                captures,
            )
        {
            output.push_str(&trim_capture(value, rule, context));
            index = end;
            continue;
        }
        let character = source[index..].chars().next().expect("valid boundary");
        output.push(character);
        index += character.len_utf8();
    }
    substitute_macros(&output, &context.macros, false)
}

fn capture_reference<'a>(
    source: &str,
    start: usize,
    input: &'a str,
    match_start: usize,
    match_end: usize,
    captures: &'a Captures<'a>,
) -> Option<(usize, &'a str)> {
    let tail = &source[start + 1..];
    if let Some(name) = tail
        .strip_prefix('<')
        .and_then(|value| value.split_once('>'))
    {
        let end = start + name.0.len() + 3;
        return Some((
            end,
            captures.name(name.0).map_or("", |value| value.as_str()),
        ));
    }
    let special = tail.as_bytes().first().copied()?;
    match special {
        b'$' => return Some((start + 2, "$")),
        b'&' => return Some((start + 2, &input[match_start..match_end])),
        b'`' => return Some((start + 2, &input[..match_start])),
        b'\'' => return Some((start + 2, &input[match_end..])),
        _ => {}
    }
    let digits = tail
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .take(2)
        .count();
    if digits == 0 {
        return None;
    }
    let end = start + 1 + digits;
    let index = source[start + 1..end].parse::<usize>().ok()?;
    Some((end, captures.get(index).map_or("", |value| value.as_str())))
}

fn trim_capture(value: &str, rule: &TextTransformRule, context: &TextTransformContext) -> String {
    rule.trim_strings
        .iter()
        .fold(value.to_owned(), |text, trim| {
            text.replace(&substitute_macros(trim, &context.macros, false), "")
        })
}

fn error(code: &'static str, message: impl Into<String>) -> LlmConfigError {
    LlmConfigError::new(code, message)
}
