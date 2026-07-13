use serde_json::Value;

use super::SillyTavernPresetKind;

pub fn detect_preset_kind(document: &Value) -> SillyTavernPresetKind {
    if is_regex_array(document) {
        return SillyTavernPresetKind::RegexScripts;
    }
    let Some(object) = document.as_object() else {
        return SillyTavernPresetKind::Unknown;
    };
    if object.get("prompts").is_some_and(Value::is_array)
        && object.get("prompt_order").is_some_and(Value::is_array)
    {
        return SillyTavernPresetKind::OpenAi;
    }
    if ["context", "instruct", "sysprompt", "reasoning", "preset"]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return SillyTavernPresetKind::Master;
    }
    if object.contains_key("story_string") {
        return SillyTavernPresetKind::Context;
    }
    if object.contains_key("input_sequence") && object.contains_key("output_sequence") {
        return SillyTavernPresetKind::Instruct;
    }
    if object.contains_key("content") && object.contains_key("name") {
        return SillyTavernPresetKind::SystemPrompt;
    }
    if ["prefix", "suffix", "separator"]
        .iter()
        .all(|key| object.contains_key(*key))
    {
        return SillyTavernPresetKind::Reasoning;
    }
    if ["temp", "top_k", "top_p", "rep_pen"]
        .iter()
        .all(|key| object.contains_key(*key))
    {
        return SillyTavernPresetKind::TextCompletion;
    }
    SillyTavernPresetKind::Unknown
}

fn is_regex_array(value: &Value) -> bool {
    value.as_array().is_some_and(|values| {
        values.iter().all(|value| {
            value.get("findRegex").is_some_and(Value::is_string)
                && value.get("replaceString").is_some_and(Value::is_string)
        })
    })
}
