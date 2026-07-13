use serde_json::{Map, Value};

use crate::llm::context::{
    ContextAssemblySpec, ContextItem, ContextPosition, ContextRole, ContextSource, TokenBudgetHint,
};

use super::super::support::{ImportParts, substitute_known_macros, warning};

pub(super) fn import_literal(
    spec: &mut ContextAssemblySpec,
    prompt: &Map<String, Value>,
    identifier: &str,
    enabled: bool,
    index: usize,
    position: ContextPosition,
    parts: &mut ImportParts,
) {
    let raw_content = prompt
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let content = substitute_known_macros(raw_content, &spec.text_transform_macros);
    let role = prompt_role(prompt, identifier, parts);
    let id = prompt_item_id(identifier, index);
    let item = ContextItem {
        id: id.clone(),
        name: prompt
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        enabled,
        requested_role: role,
        source: ContextSource::Literal {
            text: content.clone(),
        },
        position,
        order: index as i64,
        priority: 100,
        insertion_depth: 0,
        budget: TokenBudgetHint {
            max_tokens: None,
            required: true,
        },
        overflow: None,
    };
    if let Some(existing) = spec.items.iter_mut().find(|item| item.id == id) {
        *existing = item;
    } else {
        spec.items.push(item);
    }
    if content.contains("{{") {
        warning(
            &mut parts.warnings,
            "sillytavern_prompt_macros_require_binding",
            format!(
                "prompt {identifier} contains SillyTavern macros; unresolved macros remain visible in preview"
            ),
            Some(format!("prompts.{identifier}.content")),
        );
    }
}

pub(super) fn import_assistant_prefill(document: &Value, spec: &mut ContextAssemblySpec) {
    let Some(text) = document
        .get("assistant_prefill")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let item = ContextItem {
        id: "st:assistant-prefill".into(),
        name: Some("Assistant Prefill".into()),
        enabled: true,
        requested_role: ContextRole::Assistant,
        source: ContextSource::Literal { text: text.into() },
        position: ContextPosition::AssistantPrefill,
        order: 0,
        priority: 100,
        insertion_depth: 0,
        budget: TokenBudgetHint {
            max_tokens: None,
            required: true,
        },
        overflow: None,
    };
    spec.items.retain(|value| value.id != item.id);
    spec.items.push(item);
}

fn prompt_role(
    prompt: &Map<String, Value>,
    identifier: &str,
    parts: &mut ImportParts,
) -> ContextRole {
    match prompt
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("system")
    {
        "user" => ContextRole::User,
        "assistant" => ContextRole::Assistant,
        unknown => {
            if unknown != "system" {
                warning(
                    &mut parts.warnings,
                    "unknown_sillytavern_prompt_role",
                    format!("prompt {identifier} role {unknown} was mapped to system"),
                    Some(format!("prompts.{identifier}.role")),
                );
            }
            ContextRole::System
        }
    }
}

fn prompt_item_id(identifier: &str, index: usize) -> String {
    let slug: String = identifier
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || matches!(value, '-' | '_') {
                value
            } else {
                '-'
            }
        })
        .take(96)
        .collect();
    format!("st:{index}:{slug}")
}
