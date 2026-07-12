use std::collections::HashSet;

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    graph::{DraftNodeKind, GraphApplyDependencies, GraphDraft},
    llm::context::ContextAssemblyConfig,
};

use crate::{
    StorageResult,
    config::{
        rows::{load_channel_head, load_preset_head},
        tool_registry_rows::load_tool_dependency_map,
    },
};

pub(super) async fn load_llm_dependencies<C: ConnectionTrait>(
    connection: &C,
    draft: &GraphDraft,
) -> StorageResult<GraphApplyDependencies> {
    let mut channel_ids = HashSet::new();
    let mut preset_ids = HashSet::new();
    let mut tool_grants = Vec::new();
    for node in &draft.nodes {
        let DraftNodeKind::Llm { config } = &node.kind else {
            continue;
        };
        channel_ids.insert(config.model.channel_id.clone());
        if let ContextAssemblyConfig::Preset { preset_id } = &config.context {
            preset_ids.insert(preset_id.clone());
        }
        tool_grants.extend(config.tools.clone());
    }
    let mut dependencies = GraphApplyDependencies::default();
    for channel_id in channel_ids {
        let revision = load_channel_head(connection, &channel_id).await?;
        dependencies.channel_heads.insert(channel_id, revision);
    }
    for preset_id in preset_ids {
        let version = load_preset_head(connection, &preset_id).await?;
        dependencies.preset_heads.insert(preset_id, version);
    }
    dependencies.tool_descriptors = load_tool_dependency_map(connection, &tool_grants).await?;
    Ok(dependencies)
}
