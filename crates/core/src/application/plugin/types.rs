use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginUpdatePolicy {
    Manual,
    Notify,
    Automatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    UiMessageReadDisplay,
    UiMessageDecorate,
    UiArtifactRender,
    UiPanel,
    UiTheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRendererSlot {
    ConversationMessageBody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntrypoints {
    pub ui_worker: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRendererDeclaration {
    pub id: String,
    pub slot: PluginRendererSlot,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub roles: Vec<PluginMessageRole>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub minimum_host_version: Option<String>,
    pub entrypoints: PluginEntrypoints,
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,
    #[serde(default)]
    pub renderers: Vec<PluginRendererDeclaration>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub settings_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginVersionView {
    pub id: String,
    pub plugin_id: String,
    pub version: String,
    pub resolved_commit: String,
    pub tree_hash: String,
    pub manifest_hash: String,
    pub manifest: PluginManifest,
    pub installed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallationView {
    pub plugin_id: String,
    pub source_url: String,
    pub source_ref: Option<String>,
    pub credential_secret_id: Option<String>,
    pub credential_username: Option<String>,
    pub update_policy: PluginUpdatePolicy,
    pub enabled: bool,
    pub active_version: PluginVersionView,
    pub previous_versions: Vec<PluginVersionView>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCandidateView {
    pub id: String,
    pub planned_version_id: String,
    pub source_url: String,
    pub source_ref: Option<String>,
    pub credential_secret_id: Option<String>,
    pub credential_username: Option<String>,
    pub resolved_commit: String,
    pub tree_hash: String,
    pub manifest_hash: String,
    pub manifest: PluginManifest,
    pub current_version_id: Option<String>,
    pub added_permissions: Vec<PluginPermission>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntrypointView {
    pub plugin_id: String,
    pub version_id: String,
    pub content_hash: String,
    pub code: String,
}
