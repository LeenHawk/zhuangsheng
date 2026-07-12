use std::collections::BTreeSet;

use crate::llm::ir::{ContextSensitivity, ContextTrust, LlmContentPartIr, MessageRole};

use super::{
    ContextAssemblyError, ContextAssemblyInput, ContextAssemblyResult, ContextItem,
    ContextProvenance, ContextRole, ContextSource, HistoryStrategy, ResolvedContextBinding,
    ResolvedContextValue, TagMatch, WorldInfoSelector,
    candidate::{CandidateGroup, ContextCandidate},
    resolve_support::{
        authorized_data_role, binding, build_candidate, trusted_config_role, value_text,
    },
    resolve_template::template_candidate,
};

pub(super) fn resolve_items(
    input: &ContextAssemblyInput,
    items: &[ContextItem],
) -> ContextAssemblyResult<Vec<CandidateGroup>> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.enabled)
        .map(|(index, item)| resolve_item(input, item, index))
        .collect()
}

fn resolve_item(
    input: &ContextAssemblyInput,
    item: &ContextItem,
    item_index: usize,
) -> ContextAssemblyResult<CandidateGroup> {
    let candidates = match &item.source {
        ContextSource::Literal { text } => vec![literal_candidate(item, item_index, text)?],
        ContextSource::Input { path } => vec![input_candidate(input, item, item_index, path)?],
        ContextSource::Template {
            variables,
            on_missing,
            compiled,
            ..
        } => vec![template_candidate(
            input,
            item,
            item_index,
            variables,
            *on_missing,
            compiled.as_ref().ok_or_else(|| {
                ContextAssemblyError::new(
                    "context_template_not_compiled",
                    "context template has no compiled program",
                )
            })?,
        )?],
        ContextSource::History {
            binding_id,
            strategy,
        } => history_candidates(input, item, item_index, binding_id, strategy)?,
        source => data_candidates(input, item, item_index, source)?,
    };
    Ok(CandidateGroup {
        item_id: item.id.clone(),
        item_index,
        position: item.position,
        order: item.order,
        priority: item.priority,
        insertion_depth: item.insertion_depth,
        required: item.budget.required,
        max_tokens: item.budget.max_tokens,
        overflow: item.overflow.clone(),
        candidates,
        pre_action: None,
    })
}

fn literal_candidate(
    item: &ContextItem,
    item_index: usize,
    text: &str,
) -> ContextAssemblyResult<ContextCandidate> {
    let mut transformations = Vec::new();
    let role = trusted_config_role(item, &mut transformations);
    build_candidate(
        item,
        item_index,
        0,
        None,
        role,
        vec![LlmContentPartIr::Text { text: text.into() }],
        ContextProvenance {
            source_type: "literal".into(),
            source_id: item.id.clone(),
            trust: ContextTrust::TrustedConfig,
            sensitivity: ContextSensitivity::Public,
        },
        transformations,
        None,
        None,
    )
}

fn input_candidate(
    input: &ContextAssemblyInput,
    item: &ContextItem,
    item_index: usize,
    path: &str,
) -> ContextAssemblyResult<ContextCandidate> {
    let value = input.node_input.pointer(path).ok_or_else(|| {
        ContextAssemblyError::new(
            "context_input_missing",
            format!("input path did not resolve for item {}", item.id),
        )
    })?;
    let text = value_text(value)?;
    let mut transformations = Vec::new();
    let role = if item.requested_role == ContextRole::User {
        ContextRole::User
    } else {
        transformations.push("role_downgraded_to_context".into());
        ContextRole::Context
    };
    build_candidate(
        item,
        item_index,
        0,
        None,
        role,
        vec![LlmContentPartIr::Text { text }],
        ContextProvenance {
            source_type: "input".into(),
            source_id: path.into(),
            trust: ContextTrust::UserInput,
            sensitivity: ContextSensitivity::Private,
        },
        transformations,
        None,
        None,
    )
}

fn history_candidates(
    input: &ContextAssemblyInput,
    item: &ContextItem,
    item_index: usize,
    binding_id: &str,
    strategy: &HistoryStrategy,
) -> ContextAssemblyResult<Vec<ContextCandidate>> {
    let binding = binding(input, binding_id)?;
    let mut values: Vec<_> = binding
        .values
        .iter()
        .map(|value| match value {
            ResolvedContextValue::HistoryMessage { .. } => Ok(value),
            _ => Err(ContextAssemblyError::new(
                "context_binding_type_mismatch",
                format!("history binding contains data values: {binding_id}"),
            )),
        })
        .collect::<ContextAssemblyResult<_>>()?;
    values.sort_by_key(|value| history_order(value));
    if values
        .windows(2)
        .any(|pair| history_order(pair[0]) == history_order(pair[1]))
    {
        return Err(ContextAssemblyError::new(
            "context_history_order_duplicate",
            format!("history binding has duplicate stableOrder values: {binding_id}"),
        ));
    }
    if let HistoryStrategy::Recent { count } = strategy {
        let keep = usize::try_from(*count).unwrap_or(usize::MAX);
        if values.len() > keep {
            values.drain(..values.len() - keep);
        }
    }
    values
        .into_iter()
        .enumerate()
        .map(|(sub_index, value)| history_candidate(item, item_index, sub_index, value))
        .collect()
}

fn history_order(value: &ResolvedContextValue) -> u64 {
    match value {
        ResolvedContextValue::HistoryMessage { stable_order, .. } => *stable_order,
        ResolvedContextValue::Data { .. } => unreachable!(),
    }
}

fn history_candidate(
    item: &ContextItem,
    item_index: usize,
    sub_index: usize,
    value: &ResolvedContextValue,
) -> ContextAssemblyResult<ContextCandidate> {
    let ResolvedContextValue::HistoryMessage {
        message_id,
        stable_order,
        role,
        content_hash,
        content,
        provenance,
        ..
    } = value
    else {
        unreachable!()
    };
    let final_role = match role {
        MessageRole::User => ContextRole::User,
        MessageRole::Assistant => ContextRole::Assistant,
    };
    let mut candidate = build_candidate(
        item,
        item_index,
        sub_index,
        Some(*stable_order),
        final_role,
        content.clone(),
        provenance.clone(),
        Vec::new(),
        Some(content_hash.clone()),
        None,
    )?;
    candidate.id = message_id.clone();
    Ok(candidate)
}

fn data_candidates(
    input: &ContextAssemblyInput,
    item: &ContextItem,
    item_index: usize,
    source: &ContextSource,
) -> ContextAssemblyResult<Vec<ContextCandidate>> {
    let binding_id = source.binding_id().expect("binding source");
    let binding = binding(input, binding_id)?;
    validate_source_scope(source, binding)?;
    let mut data = Vec::new();
    for value in &binding.values {
        let ResolvedContextValue::Data {
            content_hash,
            content,
            provenance,
            allowed_roles,
            relevance_score_micros,
            tags,
            ..
        } = value
        else {
            return Err(ContextAssemblyError::new(
                "context_binding_type_mismatch",
                format!("data binding contains history values: {binding_id}"),
            ));
        };
        if !world_info_matches(source, tags) {
            continue;
        }
        let mut transformations = Vec::new();
        let role = authorized_data_role(
            item.requested_role,
            provenance.trust,
            allowed_roles,
            &mut transformations,
        )?;
        data.push(build_candidate(
            item,
            item_index,
            data.len(),
            None,
            role,
            content.clone(),
            provenance.clone(),
            transformations,
            Some(content_hash.clone()),
            *relevance_score_micros,
        )?);
        if source_limit(source).is_some_and(|limit| data.len() >= limit) {
            break;
        }
    }
    Ok(data)
}

fn validate_source_scope(
    source: &ContextSource,
    binding: &ResolvedContextBinding,
) -> ContextAssemblyResult<()> {
    if let ContextSource::Summary {
        scope: Some(scope), ..
    } = source
        && !scope.is_empty()
        && binding.scope != *scope
        && !binding.scope.starts_with(&format!("{scope}/"))
    {
        return Err(ContextAssemblyError::new(
            "context_binding_scope_mismatch",
            "summary scope exceeds its resolved binding",
        ));
    }
    Ok(())
}

fn world_info_matches(source: &ContextSource, tags: &[String]) -> bool {
    let ContextSource::WorldInfo {
        selector:
            WorldInfoSelector::Tags {
                tags: required,
                match_mode,
            },
        ..
    } = source
    else {
        return true;
    };
    let actual: BTreeSet<_> = tags.iter().map(String::as_str).collect();
    match match_mode {
        TagMatch::Any => required.iter().any(|tag| actual.contains(tag.as_str())),
        TagMatch::All => required.iter().all(|tag| actual.contains(tag.as_str())),
    }
}

fn source_limit(source: &ContextSource) -> Option<usize> {
    match source {
        ContextSource::ToolTrace { selector, .. } => usize::try_from(selector.max_calls).ok(),
        ContextSource::EventTrace { selector, .. } => usize::try_from(selector.limit).ok(),
        _ => None,
    }
}
