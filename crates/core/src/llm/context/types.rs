use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graph::InputSelector;

pub const CONTEXT_SEMANTIC_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextAssemblyMode {
    Chat,
    Completion,
    Structured,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAssemblySpec {
    pub id: Option<String>,
    pub name: Option<String>,
    pub mode: ContextAssemblyMode,
    #[serde(default)]
    pub items: Vec<ContextItem>,
    pub budget: Option<ContextBudgetPolicy>,
    #[serde(default)]
    pub post_process: Vec<PromptPostProcessRule>,
    pub preview: Option<PreviewPolicy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextItem {
    pub id: String,
    pub name: Option<String>,
    pub enabled: bool,
    pub requested_role: ContextRole,
    pub source: ContextSource,
    pub position: ContextPosition,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub insertion_depth: u32,
    #[serde(default)]
    pub budget: TokenBudgetHint,
    pub overflow: Option<OverflowPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextRole {
    Policy,
    System,
    Developer,
    Context,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContextSource {
    Literal {
        text: String,
    },
    Template {
        syntax: TemplateSyntax,
        template: String,
        #[serde(default)]
        variables: BTreeMap<String, TemplateVariableSource>,
        on_missing: TemplateMissingPolicy,
        compiled: Option<TemplateProgramV1>,
    },
    Input {
        path: String,
    },
    Memory {
        binding_id: String,
        view: Option<MemoryView>,
    },
    WorkingMemory {
        binding_id: String,
        path: Option<String>,
    },
    State {
        binding_id: String,
        path: Option<String>,
    },
    History {
        binding_id: String,
        strategy: HistoryStrategy,
    },
    WorldInfo {
        binding_id: String,
        selector: WorldInfoSelector,
    },
    Summary {
        binding_id: String,
        scope: Option<String>,
    },
    ToolTrace {
        binding_id: String,
        selector: ToolTraceSelector,
    },
    EventTrace {
        binding_id: String,
        selector: EventTraceSelector,
    },
    Artifact {
        binding_id: String,
        selector: Option<ArtifactSelector>,
    },
    BranchContext {
        binding_id: String,
    },
}

impl ContextSource {
    pub fn binding_id(&self) -> Option<&str> {
        match self {
            Self::Memory { binding_id, .. }
            | Self::WorkingMemory { binding_id, .. }
            | Self::State { binding_id, .. }
            | Self::History { binding_id, .. }
            | Self::WorldInfo { binding_id, .. }
            | Self::Summary { binding_id, .. }
            | Self::ToolTrace { binding_id, .. }
            | Self::EventTrace { binding_id, .. }
            | Self::Artifact { binding_id, .. }
            | Self::BranchContext { binding_id } => Some(binding_id),
            Self::Literal { .. } | Self::Template { .. } | Self::Input { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSyntax {
    ZhuangshengTemplateV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateMissingPolicy {
    Error,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TemplateVariableSource {
    Literal {
        value: Value,
    },
    Input {
        selector: InputSelector,
    },
    Binding {
        binding_id: String,
        selector: InputSelector,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateProgramV1 {
    pub syntax_version: u32,
    pub segments: Vec<TemplateSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TemplateSegment {
    Text { value: String },
    Variable { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryView {
    Summary,
    Items,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HistoryStrategy {
    All,
    Recent { count: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WorldInfoSelector {
    All,
    Tags {
        tags: Vec<String>,
        match_mode: TagMatch,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagMatch {
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTraceSelector {
    pub terminal_only: bool,
    pub max_calls: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTraceSelector {
    pub event_types: Option<Vec<String>>,
    pub after_durable_seq: Option<u64>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSelector {
    pub view: ArtifactView,
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactView {
    Text,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContextPosition {
    Start,
    BeforeHistory,
    History,
    AfterHistory,
    BeforeUserInput,
    UserInput,
    AssistantPrefill,
    End,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBudgetHint {
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OverflowPolicy {
    Drop,
    TruncateHead,
    TruncateTail,
    KeepRecent { count: Option<u32> },
    TopK { k: u32 },
    Dedupe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetPolicy {
    pub max_input_tokens: Option<u64>,
    pub strategy: Option<ContextBudgetStrategy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudgetStrategy {
    Strict,
    BestEffort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPolicy {
    pub content: PreviewContent,
    pub count: PreviewCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewContent {
    MetadataOnly,
    Authorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewCount {
    Local,
    RemoteExplicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptPostProcessRule {
    MergeAdjacentMessages,
    StrictAlternation,
    SinglePrompt,
    StripEmptyMessages,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContextAssemblyConfig {
    Preset { preset_id: String },
    Inline { spec: ContextAssemblySpec },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ContextConfigSnapshot {
    Preset {
        preset_id: String,
        version_id: String,
        version: u64,
        content_hash: String,
        semantic_policy_version: u32,
        spec: ContextAssemblySpec,
    },
    GraphInline {
        graph_revision_id: String,
        node_id: String,
        content_hash: String,
        semantic_policy_version: u32,
        spec: ContextAssemblySpec,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPresetVersion {
    pub id: String,
    pub preset_id: String,
    pub version_no: u64,
    pub semantic_policy_version: u32,
    pub spec: ContextAssemblySpec,
    pub content_hash: String,
    pub created_at: i64,
}
