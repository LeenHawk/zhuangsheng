use serde_json::Value;

use crate::{
    graph::GenerationOptionsIr,
    llm::context::{
        ContextAssemblySpec, ContextItem, ContextPosition, ContextRole, ContextSource,
        TokenBudgetHint,
    },
};

use super::super::support::{ImportParts, empty_context_spec, inactive};

pub(super) fn import_instruct(document: &Value, parts: &mut ImportParts) {
    for key in [
        "input_sequence",
        "output_sequence",
        "last_output_sequence",
        "system_sequence",
        "stop_sequence",
        "first_output_sequence",
        "output_suffix",
        "input_suffix",
        "system_suffix",
        "story_string_prefix",
        "story_string_suffix",
    ] {
        if document.get(key).is_some() {
            inactive(
                parts,
                key,
                "instruct sequence formatting is preserved but requires completion-mode framing",
            );
        }
    }
    if let Some(stop) = document
        .get("stop_sequence")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        let generation = parts.generation.get_or_insert_with(empty_generation);
        if !generation.stop.iter().any(|value| value == stop) {
            generation.stop.push(stop.into());
        }
    }
}

pub(super) fn import_reasoning(document: &Value, parts: &mut ImportParts) {
    for key in [
        "prefix",
        "suffix",
        "separator",
        "auto_parse",
        "add_to_prompts",
    ] {
        if document.get(key).is_some() {
            inactive(
                parts,
                key,
                "reasoning formatting is preserved for the reasoning projection and is not sent as a prompt today",
            );
        }
    }
}

pub(super) fn import_start_reply(document: &Value, name: &str, parts: &mut ImportParts) {
    let Some(value) = document
        .get("value")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let spec = parts
        .context_spec
        .get_or_insert_with(|| empty_context_spec(name));
    upsert(
        spec,
        ContextItem {
            id: "st:start-reply-with".into(),
            name: Some("Start Reply With".into()),
            enabled: true,
            requested_role: ContextRole::Assistant,
            source: ContextSource::Literal { text: value.into() },
            position: ContextPosition::AssistantPrefill,
            order: 0,
            priority: 100,
            insertion_depth: 0,
            budget: TokenBudgetHint {
                max_tokens: None,
                required: true,
            },
            overflow: None,
        },
    );
}

fn upsert(spec: &mut ContextAssemblySpec, item: ContextItem) {
    if let Some(existing) = spec.items.iter_mut().find(|value| value.id == item.id) {
        *existing = item;
    } else {
        spec.items.push(item);
    }
}

fn empty_generation() -> GenerationOptionsIr {
    GenerationOptionsIr {
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stop: Vec::new(),
        seed: None,
    }
}
