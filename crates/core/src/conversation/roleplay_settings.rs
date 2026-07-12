use serde::{Deserialize, Serialize};

use crate::{
    graph::{GenerationOptionsIr, LlmNodeStreaming},
    llm::LlmNodeModelRef,
};

use super::RolePlayCompatibilityView;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePlaySettingsView {
    pub profile_version: u32,
    pub revision_id: String,
    pub primary_llm_node_id: String,
    pub compatibility: RolePlayCompatibilityView,
    pub model: LlmNodeModelRef,
    pub generation: Option<GenerationOptionsIr>,
    pub streaming: Option<LlmNodeStreaming>,
    pub context_preset_id: Option<String>,
}
