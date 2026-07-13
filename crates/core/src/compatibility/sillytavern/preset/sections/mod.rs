mod context;
mod formatting;

use serde_json::Value;

use super::{
    generation::import_text_completion_generation,
    support::{ImportParts, section_kind},
};
use crate::compatibility::sillytavern::{SillyTavernPresetKind, SillyTavernResult};
use context::{import_context_template, import_system_prompt};
use formatting::{import_instruct, import_reasoning, import_start_reply};

pub(super) fn import_master(
    document: &Value,
    name: &str,
    parts: &mut ImportParts,
) -> SillyTavernResult<()> {
    for key in ["context", "instruct", "sysprompt", "preset", "reasoning"] {
        let Some(section) = document.get(key) else {
            continue;
        };
        let Some(kind) = section_kind(key) else {
            continue;
        };
        import_section(kind, section, name, parts)?;
    }
    if let Some(start) = document.get("srw") {
        import_start_reply(start, name, parts);
    }
    Ok(())
}

pub(super) fn import_section(
    kind: SillyTavernPresetKind,
    document: &Value,
    name: &str,
    parts: &mut ImportParts,
) -> SillyTavernResult<()> {
    match kind {
        SillyTavernPresetKind::SystemPrompt => import_system_prompt(document, name, parts),
        SillyTavernPresetKind::Context => import_context_template(document, name, parts),
        SillyTavernPresetKind::Instruct => import_instruct(document, parts),
        SillyTavernPresetKind::Reasoning => import_reasoning(document, parts),
        SillyTavernPresetKind::TextCompletion => import_text_completion_generation(document, parts),
        _ => {}
    }
    Ok(())
}
