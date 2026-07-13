use std::collections::BTreeMap;

use crate::{
    canonical,
    llm::{
        ir::LlmContentPartIr,
        text_transform::{
            TextTransformContext, TextTransformPlacement, TextTransformSurface,
            apply_text_transforms,
        },
    },
};

use super::{
    ContextAssemblyError, ContextAssemblyResult, ContextAssemblySpec, ContextRole, ContextSource,
    candidate::CandidateGroup,
};

pub(super) fn apply_prompt_text_transforms(
    groups: &mut [CandidateGroup],
    spec: &ContextAssemblySpec,
) -> ContextAssemblyResult<()> {
    if spec.text_transforms.is_empty() {
        return Ok(());
    }
    let depths = history_depths(groups);
    for group in groups {
        let source = &spec.items[group.item_index].source;
        for candidate in &mut group.candidates {
            let Some(placement) = placement(source, candidate.role) else {
                continue;
            };
            let depth = candidate
                .history_order
                .and_then(|order| depths.get(&order).copied())
                .or_else(|| {
                    matches!(candidate.role, ContextRole::User | ContextRole::Assistant)
                        .then_some(0)
                });
            let context = TextTransformContext {
                placement: Some(placement),
                surface: Some(TextTransformSurface::Prompt),
                depth,
                is_edit: false,
                macros: spec.text_transform_macros.clone(),
            };
            let mut applied = Vec::new();
            for part in &mut candidate.content {
                let LlmContentPartIr::Text { text } = part else {
                    continue;
                };
                let output = apply_text_transforms(text, &spec.text_transforms, &context)
                    .map_err(|error| ContextAssemblyError::new(error.code, error.message))?;
                *text = output.text;
                applied.extend(output.applied_rule_ids);
            }
            applied.sort();
            applied.dedup();
            candidate.provenance.transformations.extend(
                applied
                    .into_iter()
                    .map(|id| format!("text_transform:prompt:{id}")),
            );
            candidate.content_hash = canonical::hash(&candidate.content)?;
        }
    }
    Ok(())
}

fn history_depths(groups: &[CandidateGroup]) -> BTreeMap<u64, u32> {
    let mut orders: Vec<u64> = groups
        .iter()
        .flat_map(|group| {
            group
                .candidates
                .iter()
                .filter_map(|value| value.history_order)
        })
        .collect();
    orders.sort_unstable_by(|left, right| right.cmp(left));
    orders.dedup();
    orders
        .into_iter()
        .enumerate()
        .filter_map(|(depth, order)| u32::try_from(depth).ok().map(|depth| (order, depth)))
        .collect()
}

fn placement(source: &ContextSource, role: ContextRole) -> Option<TextTransformPlacement> {
    if matches!(source, ContextSource::WorldInfo { .. }) {
        return Some(TextTransformPlacement::WorldInfo);
    }
    match role {
        ContextRole::User => Some(TextTransformPlacement::UserInput),
        ContextRole::Assistant => Some(TextTransformPlacement::AiOutput),
        _ => None,
    }
}
