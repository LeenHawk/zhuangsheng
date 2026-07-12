use serde::Deserialize;

use crate::{
    application::memory::MemorySearchCommand,
    canonical,
    graph::MemoryToolGrant,
    memory::{
        LongTermMemoryStatus, MemoryProposalChangeInput, normalize_content,
        validate_proposal_material,
    },
};

use super::{MemoryProposalToolInput, MemoryToolBatchError, TOOL_CALL_POLICY_VERSION};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchArguments {
    scope_id: String,
    text: Option<String>,
    tags: Vec<String>,
    status: Option<LongTermMemoryStatus>,
    limit: u32,
}

pub(super) fn parse_search_arguments(
    value: serde_json::Value,
    grant: &MemoryToolGrant,
) -> Result<MemorySearchCommand, MemoryToolBatchError> {
    let arguments: SearchArguments = serde_json::from_value(value).map_err(|_| {
        MemoryToolBatchError::new(
            "memory_search_arguments_invalid",
            "search_memory arguments are invalid",
        )
    })?;
    let mut query = MemorySearchCommand {
        scope_id: arguments.scope_id,
        text: arguments.text,
        tags: arguments.tags,
        status: arguments.status,
        limit: arguments.limit,
    };
    query.tags.sort();
    query.tags.dedup();
    if !grant.scopes.contains(&query.scope_id)
        || query.limit == 0
        || query.limit > grant.max_results.unwrap_or(20)
        || query.limit > 100
        || query
            .text
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > 4096)
        || !matches!(
            query.status,
            None | Some(LongTermMemoryStatus::Active | LongTermMemoryStatus::Obsolete)
        )
    {
        return Err(MemoryToolBatchError::new(
            "memory_search_grant_denied",
            "search_memory arguments exceed the pinned grant",
        ));
    }
    Ok(query)
}

pub(super) fn normalize_proposal(
    input: &mut MemoryProposalToolInput,
    grant: &MemoryToolGrant,
) -> Result<(), MemoryToolBatchError> {
    if !grant.scopes.contains(&input.scope_id) {
        return Err(MemoryToolBatchError::new(
            "memory_proposal_grant_denied",
            "proposal scope is not granted",
        ));
    }
    validate_proposal_material(
        &input.scope_id,
        &input.reason,
        &input.evidence_refs,
        1,
        TOOL_CALL_POLICY_VERSION,
    )
    .map_err(|error| MemoryToolBatchError::new(error.code, error.message))?;
    match &mut input.change {
        MemoryProposalChangeInput::Create { content }
        | MemoryProposalChangeInput::ReplaceContent { content } => {
            *content = normalize_content(content.clone())
                .map_err(|error| MemoryToolBatchError::new(error.code, error.message))?
        }
        _ => {}
    }
    let create = matches!(input.change, MemoryProposalChangeInput::Create { .. });
    let size = canonical::to_vec(input)
        .map_err(|error| {
            MemoryToolBatchError::new("memory_proposal_arguments_invalid", error.to_string())
        })?
        .len() as u64;
    if create != input.memory_id.is_none()
        || create != input.expected_head_commit_id.is_none()
        || size > grant.max_proposal_bytes.unwrap_or(0)
    {
        return Err(MemoryToolBatchError::new(
            "memory_proposal_arguments_invalid",
            "proposal target or size is invalid",
        ));
    }
    Ok(())
}
