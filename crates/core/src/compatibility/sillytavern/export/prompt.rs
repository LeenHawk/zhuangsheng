use std::collections::HashSet;

use serde_json::{Value, json};

use crate::llm::context::{
    ContextAssemblySpec, ContextItem, ContextPosition, ContextRole, ContextSource,
};

use super::warning;
use crate::compatibility::sillytavern::SillyTavernImportWarning;

pub(super) fn export_prompts(
    spec: &ContextAssemblySpec,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> (Vec<Value>, Vec<Value>, Option<String>) {
    let mut items: Vec<_> = spec.items.iter().enumerate().collect();
    items.sort_by_key(|(index, item)| (item.order, *index));
    let mut prompts = Vec::new();
    let mut order = Vec::new();
    let mut identifiers = HashSet::new();
    let mut assistant_prefill = None;
    for (_, item) in items {
        if matches!(item.position, ContextPosition::AssistantPrefill) {
            if let Some(text) = item_text(item) {
                assistant_prefill = Some(text.to_owned());
            }
            continue;
        }
        let marker = marker_identifier(item);
        let Some(content) = marker.map(|_| "").or_else(|| item_text(item)) else {
            warning(
                warnings,
                "sillytavern_export_source",
                "unsupported context source was omitted",
                Some(item.id.clone()),
            );
            continue;
        };
        let base = marker.unwrap_or_else(|| literal_identifier(&item.id));
        let identifier = unique_identifier(base, &mut identifiers);
        prompts.push(json!({
            "identifier":identifier,
            "name":item.name.as_deref().unwrap_or(&item.id),
            "role":role_name(item.requested_role),
            "content":content,
            "marker":marker.is_some()
        }));
        order.push(json!({"identifier":identifier,"enabled":item.enabled}));
    }
    (prompts, order, assistant_prefill)
}

fn marker_identifier(item: &ContextItem) -> Option<&'static str> {
    if matches!(item.source, ContextSource::History { .. }) {
        return Some("chatHistory");
    }
    match item.id.split([':', '/']).next().unwrap_or(&item.id) {
        "character" => Some("charDescription"),
        "persona" => Some("personaDescription"),
        "world" => Some("scenario"),
        "lore" => Some("worldInfoBefore"),
        "examples" => Some("dialogueExamples"),
        "history" => Some("chatHistory"),
        _ => None,
    }
}

fn item_text(item: &ContextItem) -> Option<&str> {
    match &item.source {
        ContextSource::Literal { text } => Some(text),
        ContextSource::Template { template, .. } => Some(template),
        _ => None,
    }
}

fn literal_identifier(id: &str) -> &str {
    id.strip_prefix("st:")
        .and_then(|tail| tail.split_once(':'))
        .map_or(id, |(_, identifier)| identifier)
}

fn unique_identifier(base: &str, used: &mut HashSet<String>) -> String {
    let mut value = base.to_owned();
    let mut suffix = 2u32;
    while !used.insert(value.clone()) {
        value = format!("{base}-{suffix}");
        suffix += 1;
    }
    value
}

fn role_name(role: ContextRole) -> &'static str {
    match role {
        ContextRole::User => "user",
        ContextRole::Assistant => "assistant",
        _ => "system",
    }
}
