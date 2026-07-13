use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    graph::{AppliedGraphDefinition, DraftNodeKind},
    llm::{
        context::{ContextAssemblyConfig, ContextAssemblySpec},
        ir::{LlmContentPartIr, validate_content_parts},
        text_transform::{
            TextTransformContext, TextTransformSurface, TextTransformTarget, apply_text_transforms,
        },
    },
};

use crate::{StorageError, StorageResult, config::rows::load_preset_head};

pub(super) async fn canonical_user_content<C: ConnectionTrait>(
    connection: &C,
    definition: &AppliedGraphDefinition,
    content: &[LlmContentPartIr],
) -> StorageResult<Vec<LlmContentPartIr>> {
    let Some(spec) = roleplay_context_spec(connection, definition).await? else {
        return Ok(content.to_vec());
    };
    apply_user_content(&spec, content)
}

pub(super) fn apply_user_content(
    spec: &ContextAssemblySpec,
    content: &[LlmContentPartIr],
) -> StorageResult<Vec<LlmContentPartIr>> {
    let context = TextTransformContext {
        target: Some(TextTransformTarget::UserInput),
        surface: Some(TextTransformSurface::Canonical),
        depth: Some(0),
        is_edit: false,
        macros: spec.text_transform_macros.clone(),
    };
    let mut transformed = content.to_vec();
    for part in &mut transformed {
        let LlmContentPartIr::Text { text } = part else {
            continue;
        };
        *text = apply_text_transforms(text, &spec.text_transforms, &context)
            .map_err(|error| StorageError::InvalidArgument(error.to_string()))?
            .text;
    }
    validate_content_parts(&transformed, true).map_err(|_| {
        StorageError::InvalidArgument(
            "canonical user text transforms produced invalid conversation content".into(),
        )
    })?;
    Ok(transformed)
}

async fn roleplay_context_spec<C: ConnectionTrait>(
    connection: &C,
    definition: &AppliedGraphDefinition,
) -> StorageResult<Option<ContextAssemblySpec>> {
    let mut contexts = definition.nodes.iter().filter_map(|node| {
        let DraftNodeKind::Llm { config } = &node.kind else {
            return None;
        };
        Some(&config.context)
    });
    let Some(context) = contexts.next() else {
        return Ok(None);
    };
    if contexts.next().is_some() {
        return Ok(None);
    }
    match context {
        ContextAssemblyConfig::Inline { spec } => Ok(Some(spec.clone())),
        ContextAssemblyConfig::Preset { preset_id } => {
            Ok(Some(load_preset_head(connection, preset_id).await?.spec))
        }
    }
}
