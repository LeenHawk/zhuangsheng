mod marker;
mod prompt;

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::llm::context::{ContextAssemblySpec, ContextPosition};

use super::{
    generation::import_openai_generation,
    support::{ImportParts, compatibility_error, empty_context_spec, inactive, warning},
};
use crate::compatibility::sillytavern::SillyTavernResult;
use marker::import_marker;
use prompt::{import_assistant_prefill, import_literal};

pub(super) fn import_openai(
    document: &Value,
    name: &str,
    parts: &mut ImportParts,
) -> SillyTavernResult<()> {
    import_openai_generation(document, parts);
    mark_inactive_fields(document, parts);
    let prompts = prompt_map(document)?;
    let order = selected_order(document)?;
    let history_index = order
        .iter()
        .position(|entry| entry.get("identifier").and_then(Value::as_str) == Some("chatHistory"));
    let mut spec = parts
        .context_spec
        .take()
        .unwrap_or_else(|| empty_context_spec(name));
    spec.name = Some(name.to_owned());
    super::support::populate_role_macros(&mut spec);
    let mut used_items = HashSet::new();
    for (index, entry) in order.iter().enumerate() {
        import_order_entry(
            &mut spec,
            &prompts,
            entry,
            index,
            history_index,
            &mut used_items,
            parts,
        );
    }
    import_assistant_prefill(document, &mut spec);
    parts.context_spec = Some(spec);
    Ok(())
}

fn import_order_entry(
    spec: &mut ContextAssemblySpec,
    prompts: &HashMap<&str, &Map<String, Value>>,
    entry: &Value,
    index: usize,
    history_index: Option<usize>,
    used_items: &mut HashSet<usize>,
    parts: &mut ImportParts,
) {
    let Some(identifier) = entry.get("identifier").and_then(Value::as_str) else {
        warning(
            &mut parts.warnings,
            "invalid_sillytavern_prompt_order",
            "prompt order entry has no identifier",
            Some(format!("prompt_order.order[{index}]")),
        );
        return;
    };
    let enabled = entry
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let Some(prompt) = prompts.get(identifier) else {
        warning(
            &mut parts.warnings,
            "missing_sillytavern_prompt",
            format!("prompt order references missing prompt {identifier}"),
            Some(format!("prompt_order.order[{index}]")),
        );
        return;
    };
    let position = position_for(index, history_index);
    if prompt
        .get("marker")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        import_marker(
            spec, identifier, enabled, index, position, used_items, parts,
        );
    } else {
        import_literal(spec, prompt, identifier, enabled, index, position, parts);
    }
}

fn prompt_map(document: &Value) -> SillyTavernResult<HashMap<&str, &Map<String, Value>>> {
    let prompts = document
        .get("prompts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            compatibility_error(
                "invalid_sillytavern_openai_preset",
                "prompts must be an array",
            )
        })?;
    Ok(prompts
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|object| {
            object
                .get("identifier")
                .and_then(Value::as_str)
                .map(|id| (id, object))
        })
        .collect())
}

fn selected_order(document: &Value) -> SillyTavernResult<&Vec<Value>> {
    let orders = document
        .get("prompt_order")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            compatibility_error(
                "invalid_sillytavern_openai_preset",
                "prompt_order must be an array",
            )
        })?;
    let selected = orders
        .iter()
        .find(|value| value.get("character_id").and_then(Value::as_i64) == Some(100001))
        .or_else(|| orders.first())
        .ok_or_else(|| {
            compatibility_error("invalid_sillytavern_openai_preset", "prompt_order is empty")
        })?;
    selected
        .get("order")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            compatibility_error(
                "invalid_sillytavern_openai_preset",
                "selected prompt order has no order array",
            )
        })
}

fn position_for(index: usize, history_index: Option<usize>) -> ContextPosition {
    match history_index {
        Some(history) if index > history => ContextPosition::AfterHistory,
        Some(_) => ContextPosition::BeforeHistory,
        None => ContextPosition::Start,
    }
}

fn mark_inactive_fields(document: &Value, parts: &mut ImportParts) {
    for key in [
        "reverse_proxy",
        "proxy_password",
        "custom_url",
        "custom_include_headers",
        "custom_include_body",
        "custom_exclude_body",
    ] {
        if document.get(key).is_some_and(|value| !value.is_null()) {
            inactive(
                parts,
                key,
                "connection and credential fields are never imported from presets",
            );
        }
    }
    for key in [
        "chat_completion_source",
        "openai_model",
        "claude_model",
        "openrouter_model",
    ] {
        if document.get(key).is_some() {
            inactive(
                parts,
                key,
                "model selection remains controlled by the versioned channel and graph",
            );
        }
    }
}
