use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    graph::{GenerationOptionsIr, ProviderExtensionsIr},
    llm::{context::ContextAssemblySpec, text_transform::TextTransformRule},
};

pub const SILLYTAVERN_COMPATIBILITY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SillyTavernPresetKind {
    OpenAi,
    Master,
    Context,
    Instruct,
    SystemPrompt,
    TextCompletion,
    Reasoning,
    RegexScripts,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SillyTavernImportWarning {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SillyTavernImportPreview {
    pub compatibility_version: u32,
    pub kind: SillyTavernPresetKind,
    pub name: String,
    pub source_hash: String,
    pub context_spec: Option<ContextAssemblySpec>,
    pub generation: Option<GenerationOptionsIr>,
    pub provider_extensions: Option<ProviderExtensionsIr>,
    pub text_transforms: Vec<TextTransformRule>,
    pub inactive_fields: Vec<String>,
    pub warnings: Vec<SillyTavernImportWarning>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SillyTavernImportInput {
    pub document: Value,
    pub source_name: Option<String>,
    pub base_spec: Option<ContextAssemblySpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct SillyTavernCompatibilityError {
    pub code: &'static str,
    pub message: String,
}

impl SillyTavernCompatibilityError {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type SillyTavernResult<T> = Result<T, SillyTavernCompatibilityError>;
