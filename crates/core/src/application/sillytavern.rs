use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compatibility::sillytavern::{
    SillyTavernCompatibilityError, SillyTavernImportInput, SillyTavernImportPreview, preview_import,
};

use super::{
    ApplicationError,
    graph::{CreateRolePlayTemplateCommand, GraphRevisionView, GraphService},
    preset::{
        ContextPresetService, ContextPresetView, CreateContextPresetCommand,
        PublishContextPresetVersionCommand,
    },
};
use crate::llm::context::{ContextAssemblySpec, ContextPresetVersion};

mod regex_test;
pub use regex_test::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSillyTavernImportCommand {
    pub document: Value,
    pub source_name: Option<String>,
    pub target_preset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySillyTavernImportCommand {
    pub document: Value,
    pub source_name: Option<String>,
    pub target_preset_id: Option<String>,
    pub expected_head_version_id: Option<String>,
    pub channel_id: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SillyTavernImportResult {
    pub preview: SillyTavernImportPreview,
    pub preset: ContextPresetView,
    pub version: ContextPresetVersion,
    pub graph_revision: Option<GraphRevisionView>,
}

pub async fn preview_sillytavern_import(
    presets: &dyn ContextPresetService,
    command: PreviewSillyTavernImportCommand,
) -> Result<SillyTavernImportPreview, ApplicationError> {
    let base_spec = load_base_spec(presets, command.target_preset_id.as_deref()).await?;
    preview_import(SillyTavernImportInput {
        document: command.document,
        source_name: command.source_name,
        base_spec,
    })
    .map_err(compatibility_error)
}

pub async fn apply_sillytavern_import(
    presets: &dyn ContextPresetService,
    graphs: &dyn GraphService,
    command: ApplySillyTavernImportCommand,
) -> Result<SillyTavernImportResult, ApplicationError> {
    if command.idempotency_key.is_empty() || command.idempotency_key.len() > 112 {
        return Err(ApplicationError::InvalidArgument {
            code: "invalid_idempotency_key",
            message: "SillyTavern import idempotency key must contain 1..=112 bytes".into(),
        });
    }
    if command.target_preset_id.is_none() && command.expected_head_version_id.is_some() {
        return Err(ApplicationError::InvalidArgument {
            code: "invalid_sillytavern_import_target",
            message: "a new preset cannot have an expected head version".into(),
        });
    }
    let preview = preview_sillytavern_import(
        presets,
        PreviewSillyTavernImportCommand {
            document: command.document,
            source_name: command.source_name,
            target_preset_id: command.target_preset_id.clone(),
        },
    )
    .await?;
    let preset = match command.target_preset_id.as_ref() {
        Some(preset_id) => presets.get_context_preset(&preset_id).await?,
        None => {
            require_context_spec(&preview)?;
            presets
                .create_context_preset(CreateContextPresetCommand {
                    name: preview.name.clone(),
                    idempotency_key: format!("{}:preset", command.idempotency_key),
                })
                .await?
        }
    };
    let version = match preview.context_spec.clone() {
        Some(spec) => {
            presets
                .publish_context_preset_version(PublishContextPresetVersionCommand {
                    preset_id: preset.id.clone(),
                    expected_head_version_id: command.expected_head_version_id,
                    spec,
                    idempotency_key: format!("{}:publish", command.idempotency_key),
                })
                .await?
        }
        None => {
            existing_target_version(
                presets,
                &preset,
                command.expected_head_version_id.as_deref(),
            )
            .await?
        }
    };
    let graph_revision = match command.channel_id {
        Some(channel_id) => Some(
            graphs
                .create_roleplay_template(CreateRolePlayTemplateCommand {
                    name: preview.name.clone(),
                    channel_id,
                    preset_id: preset.id.clone(),
                    generation: preview.generation.clone(),
                    extensions: preview.provider_extensions.clone(),
                    idempotency_key: format!("{}:agent", command.idempotency_key),
                })
                .await?,
        ),
        None => None,
    };
    let preset = presets.get_context_preset(&preset.id).await?;
    Ok(SillyTavernImportResult {
        preview,
        preset,
        version,
        graph_revision,
    })
}

async fn existing_target_version(
    presets: &dyn ContextPresetService,
    preset: &ContextPresetView,
    expected_head: Option<&str>,
) -> Result<ContextPresetVersion, ApplicationError> {
    let version_id =
        preset
            .head_version_id
            .as_deref()
            .ok_or_else(|| ApplicationError::InvalidArgument {
                code: "sillytavern_import_has_no_context",
                message: "a generation-only import requires a published target ContextPreset"
                    .into(),
            })?;
    if expected_head.is_some_and(|expected| expected != version_id) {
        return Err(ApplicationError::Conflict("context_preset_head"));
    }
    presets.get_context_preset_version(version_id).await
}

async fn load_base_spec(
    presets: &dyn ContextPresetService,
    preset_id: Option<&str>,
) -> Result<Option<ContextAssemblySpec>, ApplicationError> {
    let Some(preset_id) = preset_id else {
        return Ok(None);
    };
    let preset = presets.get_context_preset(preset_id).await?;
    match preset.head_version_id {
        Some(version_id) => Ok(Some(
            presets.get_context_preset_version(&version_id).await?.spec,
        )),
        None => Ok(None),
    }
}

fn require_context_spec(
    preview: &SillyTavernImportPreview,
) -> Result<ContextAssemblySpec, ApplicationError> {
    preview
        .context_spec
        .clone()
        .ok_or_else(|| ApplicationError::InvalidArgument {
            code: "sillytavern_import_has_no_context",
            message: "this preset contains generation settings only; choose them while creating an Agent template"
                .into(),
        })
}

fn compatibility_error(error: SillyTavernCompatibilityError) -> ApplicationError {
    ApplicationError::InvalidArgument {
        code: error.code,
        message: error.message,
    }
}
