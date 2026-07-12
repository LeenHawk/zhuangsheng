use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{
    canonical,
    graph::{LlmNodeExecutionSnapshot, LlmOutputSpec, ToolGrant},
};

use super::{
    ResolvedToolDescriptor, ToolRegistrySnapshot,
    context::{ContextAssemblyOutput, ContextAssemblySnapshotConfig, ContextConfigSnapshot},
    ir::{
        LlmRequestIr, LlmTurnItemIr, MetadataValue, OpaqueContinuationRef, ResponseFormatIr,
        validate_request_ir,
    },
    request_builder_tools::{resolve_hosted_tools, resolve_tools},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRequestTool {
    pub binding_id: String,
    pub exposed_name: String,
    pub grant: ToolGrant,
    pub descriptor: ResolvedToolDescriptor,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequestBuildOutput {
    pub request: LlmRequestIr,
    pub request_digest: String,
    pub resolved_tools: Vec<ResolvedRequestTool>,
}

pub struct LlmRequestBuildInput<'a> {
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub context: &'a ContextAssemblyOutput,
    pub registry_snapshot: &'a ToolRegistrySnapshot,
    pub tool_descriptors: &'a [ResolvedToolDescriptor],
    pub transcript_tail: &'a [LlmTurnItemIr],
    pub continuation: Option<&'a OpaqueContinuationRef>,
    pub approved_hosted_bindings: &'a BTreeSet<String>,
    pub model_call_no: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct LlmRequestBuildError {
    pub code: &'static str,
    pub message: String,
}

impl LlmRequestBuildError {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub fn build_llm_request(
    input: LlmRequestBuildInput<'_>,
) -> Result<LlmRequestBuildOutput, LlmRequestBuildError> {
    validate_execution(input.execution, input.context, input.model_call_no)?;
    let (tools, resolved_tools) = resolve_tools(
        input.execution,
        input.registry_snapshot,
        input.tool_descriptors,
    )?;
    let hosted_tools = resolve_hosted_tools(input.execution, input.approved_hosted_bindings)?;
    let transcript = build_transcript(input.context, input.transcript_tail)?;
    let request_options = input.execution.request.clone().unwrap_or_default();
    let response_format = match input.execution.output.as_ref() {
        Some(LlmOutputSpec::Json { schema, strict }) => Some(ResponseFormatIr::Json {
            schema: Some(schema.clone()),
            strict: *strict,
        }),
        Some(LlmOutputSpec::Text { .. }) | None => Some(ResponseFormatIr::Text),
    };
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "graphRevisionId".into(),
        MetadataValue::String(input.execution.graph_revision_id.clone()),
    );
    metadata.insert(
        "nodeId".into(),
        MetadataValue::String(input.execution.node_id.clone()),
    );
    metadata.insert(
        "contextAssemblyDigest".into(),
        MetadataValue::String(input.context.snapshot.assembly_digest.clone()),
    );
    metadata.insert(
        "modelCallNo".into(),
        MetadataValue::Number(input.model_call_no.into()),
    );
    let request = LlmRequestIr {
        model: input.execution.operation.model_id.clone(),
        instructions: input.context.instructions.clone(),
        transcript,
        tools,
        hosted_tools,
        tool_choice: request_options.tool_choice,
        response_format,
        generation: request_options.generation,
        extensions: request_options.extensions,
        metadata,
        continuation: input.continuation.cloned(),
    };
    validate_request_ir(&request)
        .map_err(|error| LlmRequestBuildError::new(error.code, error.message))?;
    if matches!(
        request.tool_choice,
        Some(crate::graph::ToolChoiceIr::Required)
    ) && request.tools.is_empty()
        && request.hosted_tools.is_empty()
    {
        return Err(LlmRequestBuildError::new(
            "required_tool_unavailable",
            "tool choice requires a tool but the request exposes none",
        ));
    }
    let request_digest = canonical::hash(&request).map_err(|error| {
        LlmRequestBuildError::new("llm_request_digest_failed", error.to_string())
    })?;
    Ok(LlmRequestBuildOutput {
        request,
        request_digest,
        resolved_tools,
    })
}

fn validate_execution(
    execution: &LlmNodeExecutionSnapshot,
    context: &ContextAssemblyOutput,
    model_call_no: u64,
) -> Result<(), LlmRequestBuildError> {
    let channel_hash = canonical::hash(&execution.channel.spec).map_err(|error| {
        LlmRequestBuildError::new("llm_execution_snapshot_invalid", error.to_string())
    })?;
    let model_is_pinned = execution
        .channel
        .spec
        .model_catalogs
        .iter()
        .find(|catalog| catalog.operation_key == execution.operation.operation_key)
        .is_some_and(|catalog| {
            catalog
                .models
                .iter()
                .any(|model| model.id == execution.operation.model_id)
        });
    if model_call_no == 0
        || execution.schema_version != 1
        || execution.graph_revision_id.is_empty()
        || execution.node_id.is_empty()
        || execution.operation.model_id.is_empty()
        || execution.operation.channel_revision_id != execution.channel.id
        || execution.operation.operation_taxonomy_version
            != execution.channel.spec.operation_taxonomy_version
        || execution.operation.adapter_decoder_version
            != execution.channel.spec.adapter_decoder_version
        || !execution
            .channel
            .spec
            .operation_keys
            .contains(&execution.operation.operation_key)
        || channel_hash != execution.channel.content_hash
        || !model_is_pinned
    {
        return Err(LlmRequestBuildError::new(
            "llm_execution_snapshot_invalid",
            "request builder received an invalid pinned execution snapshot",
        ));
    }
    let hard_output = execution.limits.max_output_tokens.ok_or_else(|| {
        LlmRequestBuildError::new(
            "llm_output_limit_missing",
            "pinned execution snapshot has no output token limit",
        )
    })?;
    if hard_output == 0
        || execution
            .request
            .as_ref()
            .and_then(|request| request.generation.as_ref())
            .and_then(|generation| generation.max_output_tokens)
            .is_some_and(|value| value == 0 || value > hard_output)
    {
        return Err(LlmRequestBuildError::new(
            "generation_output_limit_exceeded",
            "generation output limit exceeds the pinned node hard limit",
        ));
    }
    if !context_snapshot_matches(&execution.context, &context.snapshot.config) {
        return Err(LlmRequestBuildError::new(
            "context_execution_snapshot_mismatch",
            "context assembly does not belong to the pinned execution snapshot",
        ));
    }
    Ok(())
}

fn context_snapshot_matches(
    execution: &ContextConfigSnapshot,
    assembled: &ContextAssemblySnapshotConfig,
) -> bool {
    match (execution, assembled) {
        (
            ContextConfigSnapshot::Preset {
                preset_id,
                version_id,
                version,
                content_hash,
                ..
            },
            ContextAssemblySnapshotConfig::Preset {
                preset_id: actual_preset,
                version_id: actual_version_id,
                version: actual_version,
                content_hash: actual_hash,
            },
        ) => {
            preset_id == actual_preset
                && version_id == actual_version_id
                && version == actual_version
                && content_hash == actual_hash
        }
        (
            ContextConfigSnapshot::GraphInline {
                graph_revision_id,
                node_id,
                content_hash,
                ..
            },
            ContextAssemblySnapshotConfig::GraphInline {
                graph_revision_id: actual_revision,
                node_id: actual_node,
                content_hash: actual_hash,
            },
        ) => {
            graph_revision_id == actual_revision
                && node_id == actual_node
                && content_hash == actual_hash
        }
        _ => false,
    }
}

fn build_transcript(
    context: &ContextAssemblyOutput,
    tail: &[LlmTurnItemIr],
) -> Result<Vec<LlmTurnItemIr>, LlmRequestBuildError> {
    let mut provenance = BTreeMap::new();
    for value in &context.provenance {
        if provenance.insert(value.id.as_str(), value).is_some() {
            return Err(LlmRequestBuildError::new(
                "context_provenance_duplicate",
                "context assembly contains duplicate provenance ids",
            ));
        }
    }
    let mut transcript = Vec::with_capacity(context.messages.len() + tail.len());
    for message in &context.messages {
        let source = provenance
            .get(message.provenance_id.as_str())
            .ok_or_else(|| {
                LlmRequestBuildError::new(
                    "context_provenance_missing",
                    "context message references missing provenance",
                )
            })?;
        transcript.push(LlmTurnItemIr::Message {
            id: message.id.clone(),
            role: message.role,
            content: message.content.clone(),
            provenance: Some((*source).clone()),
            placeholder: message.placeholder,
        });
    }
    transcript.extend_from_slice(tail);
    Ok(transcript)
}
