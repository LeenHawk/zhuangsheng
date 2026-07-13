use std::collections::HashSet;

use crate::llm::context::{
    ContextAssemblySpec, ContextItem, ContextPosition, ContextRole, ContextSource, HistoryStrategy,
    OverflowPolicy, TokenBudgetHint,
};

use super::super::support::{ImportParts, warning};

pub(super) fn import_marker(
    spec: &mut ContextAssemblySpec,
    identifier: &str,
    enabled: bool,
    index: usize,
    position: ContextPosition,
    used_items: &mut HashSet<usize>,
    parts: &mut ImportParts,
) {
    if let Some(item_index) = marker_item(spec, identifier, used_items) {
        let item = &mut spec.items[item_index];
        item.enabled = enabled;
        item.order = index as i64;
        if !matches!(item.source, ContextSource::History { .. }) {
            item.position = position;
        }
        used_items.insert(item_index);
        return;
    }
    if identifier == "chatHistory" {
        spec.items.push(history_item(enabled, index));
        return;
    }
    warning(
        &mut parts.warnings,
        "unresolved_sillytavern_marker",
        format!("marker {identifier} has no canonical source in the target preset"),
        Some(format!("prompts.{identifier}")),
    );
}

fn marker_item(
    spec: &ContextAssemblySpec,
    identifier: &str,
    used: &HashSet<usize>,
) -> Option<usize> {
    let aliases = marker_aliases(identifier);
    spec.items.iter().enumerate().find_map(|(index, item)| {
        if used.contains(&index) {
            return None;
        }
        let profile = item.id.split([':', '/']).next().unwrap_or(&item.id);
        (item.id == identifier || aliases.contains(&profile)).then_some(index)
    })
}

fn marker_aliases(identifier: &str) -> &'static [&'static str] {
    match identifier {
        "charDescription" | "charPersonality" => &["character"],
        "personaDescription" => &["persona"],
        "scenario" => &["world"],
        "worldInfoBefore" | "worldInfoAfter" => &["lore"],
        "dialogueExamples" => &["examples"],
        "chatHistory" => &["history"],
        _ => &[],
    }
}

fn history_item(enabled: bool, index: usize) -> ContextItem {
    ContextItem {
        id: "history".into(),
        name: Some("Chat History".into()),
        enabled,
        requested_role: ContextRole::Context,
        source: ContextSource::History {
            binding_id: "history".into(),
            strategy: HistoryStrategy::All,
        },
        position: ContextPosition::History,
        order: index as i64,
        priority: 90,
        insertion_depth: 0,
        budget: TokenBudgetHint::default(),
        overflow: Some(OverflowPolicy::KeepRecent { count: None }),
    }
}
