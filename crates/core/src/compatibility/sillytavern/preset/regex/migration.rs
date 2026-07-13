use serde_json::{Map, Value};

use crate::llm::text_transform::{TextTransformPlacement, TextTransformSurface};

use super::{
    super::{super::SillyTavernImportWarning, support::warning},
    fields::{boolean, number_array},
};

pub(super) fn placement_and_surfaces(
    object: &Map<String, Value>,
    index: usize,
    path: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> (Vec<TextTransformPlacement>, Vec<TextTransformSurface>) {
    let field = format!("{path}[{index}].placement");
    let mut raw = number_array(object.get("placement"), &field, warnings);
    let legacy_display = raw.contains(&0);
    if legacy_display {
        raw = if raw.len() == 1 {
            vec![1, 2, 3, 5, 6]
        } else {
            raw.into_iter().filter(|value| *value != 0).collect()
        };
        warning(
            warnings,
            "sillytavern_regex_legacy_display",
            "legacy MD placement was migrated to prompt and display surfaces",
            Some(field.clone()),
        );
    }
    if raw.contains(&4) {
        raw = if raw.len() == 1 {
            vec![3]
        } else {
            raw.into_iter().filter(|value| *value != 4).collect()
        };
        warning(
            warnings,
            "sillytavern_regex_legacy_sendas",
            "legacy sendAs placement was migrated to inactive slash-command placement",
            Some(field.clone()),
        );
    }
    let placements = placements(raw, &field, warnings);
    if placements.is_empty() {
        warning(
            warnings,
            "sillytavern_regex_no_placement",
            "rule has no active placement",
            Some(field.clone()),
        );
    }
    if placements.contains(&TextTransformPlacement::SlashCommand) {
        warning(
            warnings,
            "sillytavern_regex_slash_inactive",
            "STscript slash-command placement is preserved but not executed",
            Some(field),
        );
    }
    let markdown = legacy_display || boolean(object.get("markdownOnly"), false);
    let prompt = legacy_display || boolean(object.get("promptOnly"), false);
    (placements, surfaces(markdown, prompt))
}

fn placements(
    values: Vec<i64>,
    field: &str,
    warnings: &mut Vec<SillyTavernImportWarning>,
) -> Vec<TextTransformPlacement> {
    values
        .into_iter()
        .filter_map(|value| match value {
            1 => Some(TextTransformPlacement::UserInput),
            2 => Some(TextTransformPlacement::AiOutput),
            3 => Some(TextTransformPlacement::SlashCommand),
            5 => Some(TextTransformPlacement::WorldInfo),
            6 => Some(TextTransformPlacement::Reasoning),
            other => {
                warning(
                    warnings,
                    "unknown_sillytavern_regex_placement",
                    format!("unknown regex placement {other} was ignored"),
                    Some(field.to_owned()),
                );
                None
            }
        })
        .collect()
}

fn surfaces(markdown: bool, prompt: bool) -> Vec<TextTransformSurface> {
    match (markdown, prompt) {
        (false, false) => vec![TextTransformSurface::Canonical],
        (true, false) => vec![TextTransformSurface::Display],
        (false, true) => vec![TextTransformSurface::Prompt],
        (true, true) => vec![TextTransformSurface::Prompt, TextTransformSurface::Display],
    }
}
