use serde_json::Value;

use crate::llm::context::{
    ContextAssemblySpec, ContextItem, ContextPosition, ContextRole, ContextSource, TokenBudgetHint,
};

use super::super::support::{ImportParts, empty_context_spec, inactive, warning};

pub(super) fn import_system_prompt(document: &Value, name: &str, parts: &mut ImportParts) {
    let Some(content) = document.get("content").and_then(Value::as_str) else {
        warning(
            &mut parts.warnings,
            "invalid_sillytavern_system_prompt",
            "system prompt has no string content",
            Some("content".into()),
        );
        return;
    };
    let spec = parts
        .context_spec
        .get_or_insert_with(|| empty_context_spec(name));
    upsert(
        spec,
        literal_item(
            "st:system-prompt",
            document
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("System Prompt"),
            content,
            ContextPosition::Start,
            -100,
        ),
    );
    if content.contains("{{") {
        warning(
            &mut parts.warnings,
            "sillytavern_prompt_macros_require_binding",
            "system prompt contains SillyTavern macros; unresolved macros remain visible in preview",
            Some("content".into()),
        );
    }
    if let Some(post) = document
        .get("post_history")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        upsert(
            spec,
            literal_item(
                "st:post-history",
                "Post-history prompt",
                post,
                ContextPosition::AfterHistory,
                100,
            ),
        );
    }
}

pub(super) fn import_context_template(document: &Value, name: &str, parts: &mut ImportParts) {
    let Some(story) = document.get("story_string").and_then(Value::as_str) else {
        warning(
            &mut parts.warnings,
            "invalid_sillytavern_context_template",
            "context template has no story_string",
            Some("story_string".into()),
        );
        return;
    };
    let Some(spec) = parts.context_spec.as_mut() else {
        parts.context_spec = Some(empty_context_spec(name));
        inactive(
            parts,
            "story_string",
            "context template needs canonical character/persona/world bindings before its marker order can be applied",
        );
        return;
    };
    let markers = [
        ("system", "style"),
        ("wiBefore", "lore"),
        ("description", "character"),
        ("personality", "character"),
        ("scenario", "world"),
        ("wiAfter", "lore"),
        ("persona", "persona"),
    ];
    let mut matched = 0usize;
    for (marker, profile) in markers {
        let Some(position) = marker_position(story, marker) else {
            continue;
        };
        if let Some(item) = spec
            .items
            .iter_mut()
            .find(|item| item.id.split([':', '/']).next() == Some(profile))
        {
            item.order = i64::try_from(position).unwrap_or(i64::MAX);
            matched += 1;
        }
    }
    if matched == 0 {
        inactive(
            parts,
            "story_string",
            "custom Handlebars context layout cannot be represented without matching canonical items",
        );
    } else if story.contains("{{#") {
        warning(
            &mut parts.warnings,
            "sillytavern_context_layout_partial",
            "known context markers were reordered; custom Handlebars control flow remains inactive",
            Some("story_string".into()),
        );
    }
    for key in [
        "example_separator",
        "chat_start",
        "story_string_role",
        "story_string_depth",
    ] {
        if document.get(key).is_some() {
            inactive(
                parts,
                key,
                "context formatting field is preserved as inactive compatibility metadata",
            );
        }
    }
}

fn marker_position(story: &str, marker: &str) -> Option<usize> {
    story
        .find(&format!("{{{{{marker}}}}}"))
        .or_else(|| story.find(&format!("{{{{#if {marker}}}}}")))
}

fn literal_item(
    id: &str,
    name: &str,
    text: &str,
    position: ContextPosition,
    order: i64,
) -> ContextItem {
    ContextItem {
        id: id.into(),
        name: Some(name.into()),
        enabled: true,
        requested_role: ContextRole::System,
        source: ContextSource::Literal { text: text.into() },
        position,
        order,
        priority: 100,
        insertion_depth: 0,
        budget: TokenBudgetHint {
            max_tokens: None,
            required: true,
        },
        overflow: None,
    }
}

fn upsert(spec: &mut ContextAssemblySpec, item: ContextItem) {
    if let Some(existing) = spec.items.iter_mut().find(|value| value.id == item.id) {
        *existing = item;
    } else {
        spec.items.push(item);
    }
}
