use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    conversation::{
        RolePlayCompatibilityAnalysis, RolePlayCompatibilityView, RolePlayGraphOptionView,
        RolePlaySettingsView, analyze_roleplay_compatibility,
    },
    graph::{AppliedGraphDefinition, DraftNodeKind},
    llm::context::ContextAssemblyConfig,
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::apply::load_revision;

impl SqliteStore {
    pub async fn list_roleplay_graph_options(&self) -> StorageResult<Vec<RolePlayGraphOptionView>> {
        let rows = self.db.query_all_raw(sql(
            "SELECT g.id AS graph_id, g.name AS graph_name, r.id AS revision_id FROM graphs g JOIN graph_revisions r ON r.graph_id = g.id AND r.revision_no = (SELECT MAX(latest.revision_no) FROM graph_revisions latest WHERE latest.graph_id = g.id) ORDER BY g.updated_at DESC, g.id",
            vec![],
        )).await?;
        let mut options = Vec::with_capacity(rows.len());
        for row in rows {
            let revision_id: String = row.try_get("", "revision_id")?;
            let revision = load_revision(&self.db, &revision_id).await?;
            let analysis = analyze_with_preset(&self.db, &revision.definition).await?;
            options.push(RolePlayGraphOptionView {
                graph_id: row.try_get("", "graph_id")?,
                graph_name: row.try_get("", "graph_name")?,
                revision_id,
                revision_no: revision.revision_no,
                reply_output_keys: analysis.reply_output_keys,
                primary_llm_node_id: analysis.primary_llm_node_id,
                compatibility: analysis.compatibility,
            });
        }
        Ok(options)
    }

    pub async fn get_roleplay_compatibility(
        &self,
        revision_id: &str,
    ) -> StorageResult<RolePlayCompatibilityView> {
        let revision = load_revision(&self.db, revision_id).await?;
        Ok(analyze_with_preset(&self.db, &revision.definition)
            .await?
            .compatibility)
    }

    pub async fn get_roleplay_settings(
        &self,
        revision_id: &str,
    ) -> StorageResult<RolePlaySettingsView> {
        let revision = load_revision(&self.db, revision_id).await?;
        let analysis = analyze_with_preset(&self.db, &revision.definition).await?;
        let mut nodes = revision
            .definition
            .nodes
            .iter()
            .filter_map(|node| match &node.kind {
                DraftNodeKind::Llm { config } => Some((node.id.as_str(), config.as_ref())),
                _ => None,
            });
        let (node_id, config) = nodes.next().ok_or_else(settings_unavailable)?;
        if nodes.next().is_some() {
            return Err(settings_unavailable());
        }
        Ok(RolePlaySettingsView {
            profile_version: 1,
            revision_id: revision_id.into(),
            primary_llm_node_id: node_id.into(),
            compatibility: analysis.compatibility,
            model: config.model.clone(),
            generation: config
                .request
                .as_ref()
                .and_then(|request| request.generation.clone()),
            streaming: config.streaming.clone(),
            context_preset_id: match &config.context {
                ContextAssemblyConfig::Preset { preset_id } => Some(preset_id.clone()),
                ContextAssemblyConfig::Inline { .. } => None,
            },
        })
    }
}

fn settings_unavailable() -> StorageError {
    StorageError::InvalidArgument("role-play settings require exactly one LLM node".into())
}

async fn analyze_with_preset<C: ConnectionTrait>(
    connection: &C,
    definition: &AppliedGraphDefinition,
) -> StorageResult<RolePlayCompatibilityAnalysis> {
    let llm_configs: Vec<_> = definition
        .nodes
        .iter()
        .filter_map(|node| match &node.kind {
            DraftNodeKind::Llm { config } => Some(config.as_ref()),
            _ => None,
        })
        .collect();
    if llm_configs.len() != 1 {
        return Ok(analyze_roleplay_compatibility(definition, None));
    }
    let preset_id = match &llm_configs[0].context {
        ContextAssemblyConfig::Preset { preset_id } => Some(preset_id.as_str()),
        ContextAssemblyConfig::Inline { .. } => None,
    };
    let preset = match preset_id {
        Some(id) => match crate::config::rows::load_preset_head(connection, id).await {
            Ok(version) => Some(version.spec),
            Err(StorageError::Conflict("context_preset_has_no_version")) => None,
            Err(error) => return Err(error),
        },
        None => None,
    };
    Ok(analyze_roleplay_compatibility(definition, preset.as_ref()))
}
