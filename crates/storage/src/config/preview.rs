use zhuangsheng_core::{
    application::preset::{ContextPresetPreviewView, PreviewContextPresetCommand, preview_items},
    canonical,
    llm::context::{
        ContextAssemblyInput, ContextConfigSnapshot, ContextCountSource, EstimateTokenCounter,
        PreviewContent, ResolvedContextBinding, assemble_context,
    },
};

use crate::{SqliteStore, StorageError, StorageResult};

impl SqliteStore {
    pub async fn preview_context_preset_view(
        &self,
        command: PreviewContextPresetCommand,
    ) -> StorageResult<ContextPresetPreviewView> {
        if command.preset_id.is_empty() || command.preset_id.len() > 128 {
            return Err(StorageError::InvalidArgument(
                "context preset id is invalid".into(),
            ));
        }
        if command.budget.count_source != ContextCountSource::Estimate {
            return Err(StorageError::InvalidArgument(
                "preset preview currently supports estimate count only".into(),
            ));
        }
        let version = match command.version_id {
            Some(version_id) => self.get_context_preset_version(&version_id).await?,
            None => self.get_context_preset_head(&command.preset_id).await?,
        };
        if version.preset_id != command.preset_id {
            return Err(StorageError::InvalidArgument(
                "context preset version does not belong to the preset".into(),
            ));
        }
        let mut bindings = command.sample_bindings;
        for binding_id in version
            .spec
            .items
            .iter()
            .filter_map(|item| item.source.binding_id())
        {
            bindings
                .entry(binding_id.into())
                .or_insert_with(|| empty_binding(binding_id));
        }
        let read_set_digest = canonical::hash(&bindings)?;
        let output = assemble_context(
            &ContextAssemblyInput {
                node_input: command.node_input,
                config: ContextConfigSnapshot::Preset {
                    preset_id: version.preset_id.clone(),
                    version_id: version.id.clone(),
                    version: version.version_no,
                    content_hash: version.content_hash.clone(),
                    semantic_policy_version: version.semantic_policy_version,
                    spec: version.spec.clone(),
                },
                bindings,
                budget: command.budget,
                read_set_ref: format!("context-preview:{}:sample-bindings", version.id),
                read_set_digest,
                allow_sensitive: false,
            },
            &EstimateTokenCounter,
        )?;
        Ok(ContextPresetPreviewView {
            preset_id: version.preset_id,
            version_id: version.id,
            content_mode: PreviewContent::MetadataOnly,
            count_source: output.budget_report.count_source,
            items: preview_items(&version.spec, &output.budget_report),
            budget_report: output.budget_report,
            snapshot: output.snapshot,
        })
    }
}

fn empty_binding(binding_id: &str) -> ResolvedContextBinding {
    ResolvedContextBinding {
        binding_id: binding_id.into(),
        scope: "preview-sample".into(),
        version: "unresolved".into(),
        values: Vec::new(),
        template_value: None,
        template_provenance: None,
    }
}
