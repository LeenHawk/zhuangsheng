use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    application::{ApplicationError, preset::ContextPresetService},
    compatibility::sillytavern::{
        TextTransformContext, TextTransformPlacement, TextTransformSurface, apply_text_transforms,
    },
};

use super::{PreviewSillyTavernImportCommand, preview_sillytavern_import};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSillyTavernRegexCommand {
    pub document: Value,
    pub source_name: Option<String>,
    pub target_preset_id: Option<String>,
    pub input: String,
    pub placement: TextTransformPlacement,
    pub surface: TextTransformSurface,
    pub depth: Option<u32>,
    #[serde(default)]
    pub is_edit: bool,
    #[serde(default)]
    pub macros: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SillyTavernRegexTestResult {
    pub text: String,
    pub applied_rule_ids: Vec<String>,
}

pub async fn test_sillytavern_regex(
    presets: &dyn ContextPresetService,
    command: TestSillyTavernRegexCommand,
) -> Result<SillyTavernRegexTestResult, ApplicationError> {
    let preview = preview_sillytavern_import(
        presets,
        PreviewSillyTavernImportCommand {
            document: command.document,
            source_name: command.source_name,
            target_preset_id: command.target_preset_id,
        },
    )
    .await?;
    let mut macros = preview
        .context_spec
        .as_ref()
        .map(|spec| spec.text_transform_macros.clone())
        .unwrap_or_default();
    macros.extend(command.macros);
    let output = apply_text_transforms(
        &command.input,
        &preview.text_transforms,
        &TextTransformContext {
            placement: Some(command.placement),
            surface: Some(command.surface),
            depth: command.depth,
            is_edit: command.is_edit,
            macros,
        },
    )
    .map_err(|error| ApplicationError::InvalidArgument {
        code: error.code,
        message: error.message,
    })?;
    Ok(SillyTavernRegexTestResult {
        text: output.text,
        applied_rule_ids: output.applied_rule_ids,
    })
}
