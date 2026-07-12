use std::collections::HashSet;

use crate::{ValidationIssue, selector};

use super::{
    FinalValueSource, InputSelector, LlmNodeConfig, MemoryToolCapability, StaticContextWriteOp,
    StaticMemoryReadSource,
};

pub(super) fn validate_llm_memory(
    config: &LlmNodeConfig,
    node_id: &str,
    input_names: &HashSet<String>,
    output_names: &HashSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(memory) = &config.memory else {
        return;
    };
    if memory.node.reads.len() > 128 || memory.node.working_writes.len() > 128 {
        issues.push(issue(
            "llm_memory_binding_limit",
            node_id,
            "LLM memory bindings exceed the phase-one limit",
        ));
        return;
    }
    let mut ids = HashSet::new();
    let mut aliases = HashSet::new();
    for read in &memory.node.reads {
        let invalid_common = !valid_name(&read.id)
            || !valid_name(&read.alias)
            || !ids.insert(read.id.as_str())
            || !aliases.insert(read.alias.as_str())
            || read.max_bytes == 0
            || read.max_bytes > 16 * 1024 * 1024
            || read.limit.is_some_and(|value| value == 0 || value > 10_000);
        let invalid_source = match &read.source {
            StaticMemoryReadSource::WorkingContext { scope, path } => {
                scope.trim().is_empty() || !valid_pointer(path)
            }
            StaticMemoryReadSource::ConversationHistory { scope } => {
                scope != "run-context" || read.limit.is_some() || !read.required
            }
            StaticMemoryReadSource::LongTermMemory { scope, query } => {
                scope.trim().is_empty()
                    || query.as_ref().is_some_and(|query| {
                        query.text.len() > 16 * 1024
                            || query.tags.len() > 128
                            || query
                                .tags
                                .iter()
                                .any(|tag| tag.trim().is_empty() || tag.len() > 64)
                    })
            }
            StaticMemoryReadSource::Artifact { .. } => {
                issues.push(issue(
                    "unsupported_static_artifact_read",
                    node_id,
                    "artifact static reads require the artifact store implementation",
                ));
                false
            }
        };
        if invalid_common || invalid_source {
            issues.push(issue(
                "invalid_llm_memory_read",
                node_id,
                "LLM static memory read is invalid or duplicated",
            ));
        }
    }
    let read_aliases: HashSet<_> = memory
        .node
        .reads
        .iter()
        .map(|read| read.alias.as_str())
        .collect();
    let mut write_ids = HashSet::new();
    for write in &memory.node.working_writes {
        let value_valid = match (&write.op, &write.value_from) {
            (StaticContextWriteOp::Remove, None) => true,
            (StaticContextWriteOp::Remove, Some(_)) | (_, None) => false,
            (_, Some(value)) => {
                valid_name(&value.source_name)
                    && selector::validate(&value.selector).is_ok()
                    && match value.source {
                        FinalValueSource::Input => input_names.contains(&value.source_name),
                        FinalValueSource::Output => output_names.contains(&value.source_name),
                        FinalValueSource::Binding => {
                            read_aliases.contains(value.source_name.as_str())
                        }
                    }
            }
        };
        if !valid_name(&write.id)
            || !write_ids.insert(write.id.as_str())
            || write.target_scope != "run-context"
            || !valid_pointer(&write.path)
            || !value_valid
        {
            issues.push(issue(
                "invalid_llm_memory_write",
                node_id,
                "LLM working-context write binding is invalid or duplicated",
            ));
        }
    }
    let mut capabilities = HashSet::new();
    for tool in &memory.tools {
        if !capabilities.insert(tool.capability)
            || tool.scopes.is_empty()
            || tool
                .scopes
                .iter()
                .any(|scope| scope.trim().is_empty() || scope.len() > 256)
            || match tool.capability {
                MemoryToolCapability::SearchMemory => {
                    tool.max_results
                        .is_none_or(|value| value == 0 || value > 1_000)
                        || tool.max_proposal_bytes.is_some()
                }
                MemoryToolCapability::ProposeMemoryChange => {
                    tool.max_proposal_bytes
                        .is_none_or(|value| value == 0 || value > 16 * 1024 * 1024)
                        || tool.max_results.is_some()
                }
            }
        {
            issues.push(issue(
                "invalid_llm_memory_tool_grant",
                node_id,
                "LLM memory tool grant is invalid or duplicated",
            ));
        }
    }
}

fn valid_pointer(value: &str) -> bool {
    selector::validate(&InputSelector::JsonPointer {
        pointer: value.into(),
    })
    .is_ok()
}

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128
}

fn issue(code: &'static str, node_id: &str, message: &str) -> ValidationIssue {
    ValidationIssue::error(code, format!("/nodes/{node_id}/memory"), message)
}
