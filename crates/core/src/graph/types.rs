use serde::{Deserialize, Serialize};

use crate::schema::{JsonSchemaSpec, SchemaCompilationDraft};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDraft {
    pub graph_id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub nodes: Vec<DraftGraphNode>,
    #[serde(default)]
    pub edges: Vec<DraftGraphEdge>,
    pub run_input_schema: Option<JsonSchemaSpec>,
    #[serde(default)]
    pub output_contract: Vec<GraphOutputContractEntry>,
    pub limits: Option<DraftRunLimits>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftGraphNode {
    pub id: String,
    pub name: Option<String>,
    pub is_entry: Option<bool>,
    #[serde(default)]
    pub inputs: Vec<InputPortDefinition>,
    #[serde(default)]
    pub outputs: Vec<OutputPortDefinition>,
    pub timeout_ms: Option<u64>,
    #[serde(flatten)]
    pub kind: DraftNodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DraftNodeKind {
    Input {
        #[serde(default)]
        run_input_selector: InputSelector,
    },
    Output {
        output_key: String,
    },
    Router {
        dsl_version: String,
        #[serde(default)]
        rules: Vec<RouterRule>,
        #[serde(default)]
        match_mode: RouterMatchMode,
        #[serde(default)]
        default_outputs: Vec<String>,
        payload_port: Option<String>,
        limits: Option<RouterLimits>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub name: Option<String>,
    pub is_entry: bool,
    pub inputs: Vec<InputPortDefinition>,
    pub outputs: Vec<OutputPortDefinition>,
    pub timeout_ms: Option<u64>,
    #[serde(flatten)]
    pub kind: DraftNodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputPortDefinition {
    pub name: String,
    pub schema: Option<JsonSchemaSpec>,
    #[serde(default)]
    pub binding: ConsumerInputBinding,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPortDefinition {
    pub name: String,
    pub schema: Option<JsonSchemaSpec>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerInputBinding {
    #[serde(default)]
    pub selector: InputSelector,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputSelector {
    #[default]
    WholeValue,
    JsonPointer {
        pointer: String,
    },
    JsonPath {
        path: String,
        result: SelectorResult,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorResult {
    One,
    Many,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphOutputRef {
    pub node_id: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphInputRef {
    pub node_id: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftGraphEdge {
    pub id: Option<String>,
    pub from: GraphOutputRef,
    pub to: GraphInputRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub id: String,
    pub from: GraphOutputRef,
    pub to: GraphInputRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphOutputContractEntry {
    pub key: String,
    pub schema: Option<JsonSchemaSpec>,
    pub collection: OutputCollection,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputCollection {
    Single,
    Append,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterRule {
    pub id: String,
    pub when: String,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterMatchMode {
    #[default]
    First,
    All,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterLimits {
    pub max_visits_per_run: Option<u64>,
    pub timeout_ms_per_run: Option<u64>,
    pub max_read_reconciles: Option<u64>,
    #[serde(default)]
    pub on_limit_outputs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRunLimits {
    pub max_node_activations: Option<u64>,
    pub max_attempts_per_activation: Option<u64>,
    pub max_total_queue_values: Option<u64>,
    pub max_pending_queue_values: Option<u64>,
    pub max_open_waits: Option<u64>,
    pub max_coordinator_buffered_values: Option<u64>,
    pub max_run_wall_clock_ms: Option<u64>,
    pub max_value_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLimits {
    pub max_node_activations: u64,
    pub max_attempts_per_activation: u64,
    pub max_total_queue_values: u64,
    pub max_pending_queue_values: u64,
    pub max_open_waits: u64,
    pub max_coordinator_buffered_values: u64,
    pub max_run_wall_clock_ms: u64,
    pub max_value_bytes: u64,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_node_activations: 10_000,
            max_attempts_per_activation: 8,
            max_total_queue_values: 100_000,
            max_pending_queue_values: 10_000,
            max_open_waits: 128,
            max_coordinator_buffered_values: 10_000,
            max_run_wall_clock_ms: 24 * 60 * 60 * 1000,
            max_value_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedGraphDefinition {
    pub schema_version: u32,
    pub graph_id: String,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub run_input_schema: Option<JsonSchemaSpec>,
    pub output_contract: Vec<GraphOutputContractEntry>,
    pub limits: RunLimits,
    pub schema_semantics: Vec<SchemaSemanticDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSemanticDigest {
    pub canonical_document_hash: String,
    pub schema_hash: String,
    pub compiler_id: String,
    pub compiler_version: String,
    pub payload_format_version: u32,
    pub compiled_payload_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedGraph {
    pub definition: AppliedGraphDefinition,
    pub content_hash: String,
    pub schemas: Vec<SchemaCompilationDraft>,
    pub warnings: Vec<crate::ValidationIssue>,
}
