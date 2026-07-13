use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTransformScope {
    Global,
    Character,
    Preset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTransformPlacement {
    UserInput,
    AiOutput,
    SlashCommand,
    WorldInfo,
    Reasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTransformSurface {
    Canonical,
    Prompt,
    Display,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegexMacroMode {
    None,
    Raw,
    Escaped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextTransformRule {
    pub id: String,
    pub name: String,
    pub scope: TextTransformScope,
    pub order: u32,
    pub find_regex: String,
    pub replace_string: String,
    #[serde(default)]
    pub trim_strings: Vec<String>,
    #[serde(default)]
    pub placements: Vec<TextTransformPlacement>,
    #[serde(default)]
    pub surfaces: Vec<TextTransformSurface>,
    pub disabled: bool,
    pub run_on_edit: bool,
    pub macro_mode: RegexMacroMode,
    pub min_depth: Option<i32>,
    pub max_depth: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextTransformContext {
    pub placement: Option<TextTransformPlacement>,
    pub surface: Option<TextTransformSurface>,
    pub depth: Option<u32>,
    pub is_edit: bool,
    pub macros: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextTransformOutput {
    pub text: String,
    pub applied_rule_ids: Vec<String>,
}
