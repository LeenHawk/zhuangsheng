use serde::{Deserialize, Serialize};

use crate::{
    application::{ApplicationError, graph::GraphService, preset::ContextPresetService},
    compatibility::sillytavern::{
        SillyTavernCompatibilityError, SillyTavernExportBundle, export_sillytavern_bundle,
    },
    graph::{DraftNodeKind, GenerationOptionsIr, ProviderExtensionsIr},
    llm::context::ContextAssemblyConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSillyTavernCommand {
    pub preset_version_id: String,
    pub graph_revision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SillyTavernVersionExport {
    pub source_preset_version_id: String,
    pub source_graph_revision_id: Option<String>,
    pub bundle: SillyTavernExportBundle,
}

pub async fn export_sillytavern(
    presets: &dyn ContextPresetService,
    graphs: &dyn GraphService,
    command: ExportSillyTavernCommand,
) -> Result<SillyTavernVersionExport, ApplicationError> {
    let version = presets
        .get_context_preset_version(&command.preset_version_id)
        .await?;
    let preset = presets.get_context_preset(&version.preset_id).await?;
    let (generation, extensions) = match command.graph_revision_id.as_deref() {
        Some(revision_id) => graph_request(graphs, revision_id, &version.preset_id).await?,
        None => (None, None),
    };
    let bundle = export_sillytavern_bundle(
        &preset.name,
        &version.spec,
        generation.as_ref(),
        extensions.as_ref(),
    )
    .map_err(compatibility_error)?;
    Ok(SillyTavernVersionExport {
        source_preset_version_id: version.id,
        source_graph_revision_id: command.graph_revision_id,
        bundle,
    })
}

async fn graph_request(
    graphs: &dyn GraphService,
    revision_id: &str,
    preset_id: &str,
) -> Result<(Option<GenerationOptionsIr>, Option<ProviderExtensionsIr>), ApplicationError> {
    let revision = graphs.get_graph_revision(revision_id).await?;
    let mut candidates = revision.definition.nodes.iter().filter_map(|node| {
        let DraftNodeKind::Llm { config } = &node.kind else {
            return None;
        };
        matches!(&config.context, ContextAssemblyConfig::Preset { preset_id: id } if id == preset_id)
            .then_some(config)
    });
    let config = candidates
        .next()
        .filter(|_| candidates.next().is_none())
        .ok_or_else(|| ApplicationError::InvalidArgument {
            code: "invalid_sillytavern_export_graph",
            message: "graph revision must contain exactly one LLM node using the exported preset"
                .into(),
        })?;
    Ok(config.request.as_ref().map_or((None, None), |request| {
        (request.generation.clone(), request.extensions.clone())
    }))
}

fn compatibility_error(error: SillyTavernCompatibilityError) -> ApplicationError {
    ApplicationError::InvalidArgument {
        code: error.code,
        message: error.message,
    }
}
