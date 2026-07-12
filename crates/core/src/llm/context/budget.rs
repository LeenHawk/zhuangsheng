use std::collections::BTreeSet;

use super::{
    ContextAssemblyError, ContextAssemblyResult, ContextBudgetAction, ContextBudgetInput,
    ContextBudgetItemReport, ContextBudgetPolicy, ContextBudgetReport, ContextBudgetStrategy,
    ContextPosition, ContextTokenCounter, OverflowPolicy, budget_trim::apply_overflow,
    candidate::CandidateGroup,
};

pub(super) fn apply_budget(
    groups: &mut [CandidateGroup],
    input: &ContextBudgetInput,
    policy: &ContextBudgetPolicy,
    counter: &dyn ContextTokenCounter,
) -> ContextAssemblyResult<ContextBudgetReport> {
    apply_pre_budget_transforms(groups)?;
    for group in groups.iter_mut() {
        for candidate in &mut group.candidates {
            candidate.token_count = counter.count(candidate.role, &candidate.content)?;
        }
    }
    let available = input
        .context_window_tokens
        .checked_sub(input.reserved_output_tokens)
        .and_then(|value| value.checked_sub(input.safety_margin_tokens))
        .ok_or_else(|| budget_error("reserved tokens exceed the context window"))?;
    let available = policy
        .max_input_tokens
        .map_or(available, |limit| available.min(limit));
    let capacity = available
        .checked_sub(input.fixed_request_tokens)
        .ok_or_else(|| budget_error("fixed request tokens exceed the input budget"))?;
    let mut used = 0u64;
    let mut reports = Vec::with_capacity(groups.len());
    for group in groups.iter_mut().filter(|group| group.required) {
        if group.candidates.is_empty() {
            return Err(ContextAssemblyError::new(
                "context_required_item_missing",
                format!(
                    "required context item resolved no values: {}",
                    group.item_id
                ),
            ));
        }
        let total = group_tokens(group)?;
        if group.max_tokens.is_some_and(|limit| total > limit) {
            return Err(budget_error(format!(
                "required context item exceeds its maxTokens: {}",
                group.item_id
            )));
        }
        used = used
            .checked_add(total)
            .filter(|value| *value <= capacity)
            .ok_or_else(|| budget_error("required context items exceed the input budget"))?;
        include_all(group);
        reports.push(report(group, true, total, ContextBudgetAction::Kept, None));
    }
    let mut optional: Vec<_> = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| !group.required && !group.candidates.is_empty())
        .map(|(index, group)| (index, group.priority, display_key(group)))
        .collect();
    reports.extend(
        groups
            .iter()
            .filter(|group| !group.required && group.candidates.is_empty())
            .map(|group| {
                report(
                    group,
                    false,
                    0,
                    pre_action(group).unwrap_or(ContextBudgetAction::Dropped),
                    Some("context item resolved no values".into()),
                )
            }),
    );
    optional.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.2.cmp(&right.2)));
    let strategy = policy.strategy.unwrap_or(ContextBudgetStrategy::Strict);
    for (index, _, _) in optional {
        let group = &mut groups[index];
        let remaining = capacity.saturating_sub(used);
        let cap = group
            .max_tokens
            .map_or(remaining, |limit| remaining.min(limit));
        let total = group_tokens(group)?;
        if total <= cap {
            include_all(group);
            used = used
                .checked_add(total)
                .ok_or_else(|| budget_error("assembled token count overflow"))?;
            reports.push(report(
                group,
                true,
                total,
                pre_action(group).unwrap_or(ContextBudgetAction::Kept),
                None,
            ));
            continue;
        }
        let outcome = apply_overflow(group, cap, strategy, counter)?;
        used = used
            .checked_add(outcome.tokens)
            .ok_or_else(|| budget_error("assembled token count overflow"))?;
        reports.push(report(
            group,
            outcome.included,
            outcome.tokens,
            outcome.action,
            Some(outcome.reason),
        ));
    }
    reports.sort_by_key(|item| {
        groups
            .iter()
            .position(|group| group.item_id == item.item_id)
            .unwrap_or(usize::MAX)
    });
    Ok(ContextBudgetReport {
        available_input_tokens: available,
        fixed_request_tokens: input.fixed_request_tokens,
        assembled_tokens: used,
        count_source: input.count_source,
        items: reports,
    })
}

fn apply_pre_budget_transforms(groups: &mut [CandidateGroup]) -> ContextAssemblyResult<()> {
    for group in groups.iter_mut() {
        match group.overflow.as_ref() {
            Some(OverflowPolicy::TopK { k }) => {
                if group
                    .candidates
                    .iter()
                    .any(|candidate| candidate.relevance_score_micros.is_none())
                {
                    return Err(ContextAssemblyError::new(
                        "context_top_k_score_missing",
                        format!("top_k item has a value without a score: {}", group.item_id),
                    ));
                }
                group.candidates.sort_by(|left, right| {
                    right
                        .relevance_score_micros
                        .cmp(&left.relevance_score_micros)
                        .then_with(|| left.sub_index.cmp(&right.sub_index))
                        .then_with(|| left.id.cmp(&right.id))
                });
                let keep = usize::try_from(*k).unwrap_or(usize::MAX);
                let truncated = group.candidates.len() > keep;
                if truncated {
                    group.candidates.truncate(keep);
                    group.pre_action = Some("top_k");
                }
                for (index, candidate) in group.candidates.iter_mut().enumerate() {
                    candidate.sub_index = index;
                    if truncated {
                        candidate.provenance.transformations.push("top_k".into());
                    }
                }
            }
            Some(OverflowPolicy::KeepRecent { count: Some(count) }) => {
                let keep = usize::try_from(*count).unwrap_or(usize::MAX);
                if group.candidates.len() > keep {
                    group.candidates.drain(..group.candidates.len() - keep);
                    group.pre_action = Some("keep_recent");
                    for candidate in &mut group.candidates {
                        candidate
                            .provenance
                            .transformations
                            .push("keep_recent".into());
                    }
                }
            }
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    let mut display: Vec<_> = groups.iter().enumerate().collect();
    display.sort_by_key(|(_, group)| display_key(group));
    let mut duplicate_ids = BTreeSet::new();
    for (_, group) in display {
        if group.overflow == Some(OverflowPolicy::Dedupe) {
            for candidate in &group.candidates {
                if !seen.insert(candidate.content_hash.clone()) {
                    duplicate_ids.insert((group.item_index, candidate.id.clone()));
                }
            }
        } else {
            seen.extend(
                group
                    .candidates
                    .iter()
                    .map(|candidate| candidate.content_hash.clone()),
            );
        }
    }
    for group in groups.iter_mut() {
        let before = group.candidates.len();
        let item_index = group.item_index;
        group
            .candidates
            .retain(|candidate| !duplicate_ids.contains(&(item_index, candidate.id.clone())));
        if group.candidates.len() != before {
            group.pre_action = Some("dedupe");
            for candidate in &mut group.candidates {
                candidate.provenance.transformations.push("dedupe".into());
            }
        }
    }
    Ok(())
}

fn include_all(group: &mut CandidateGroup) {
    for candidate in &mut group.candidates {
        candidate.included = true;
    }
}

fn group_tokens(group: &CandidateGroup) -> ContextAssemblyResult<u64> {
    group.candidates.iter().try_fold(0u64, |total, candidate| {
        total
            .checked_add(candidate.token_count)
            .ok_or_else(|| budget_error("context token count overflow"))
    })
}

fn report(
    group: &CandidateGroup,
    included: bool,
    token_count: u64,
    action: ContextBudgetAction,
    reason: Option<String>,
) -> ContextBudgetItemReport {
    ContextBudgetItemReport {
        item_id: group.item_id.clone(),
        included,
        token_count,
        action,
        reason,
    }
}

fn pre_action(group: &CandidateGroup) -> Option<ContextBudgetAction> {
    match group.pre_action {
        Some("dedupe") => Some(ContextBudgetAction::Deduped),
        Some("top_k" | "keep_recent") => Some(ContextBudgetAction::Truncated),
        _ => None,
    }
}

fn display_key(group: &CandidateGroup) -> (u8, i64, usize, String) {
    (
        match group.position {
            ContextPosition::Start => 0,
            ContextPosition::BeforeHistory => 1,
            ContextPosition::History => 2,
            ContextPosition::AfterHistory => 3,
            ContextPosition::BeforeUserInput => 4,
            ContextPosition::UserInput => 5,
            ContextPosition::AssistantPrefill => 6,
            ContextPosition::End => 7,
        },
        group.order,
        group.item_index,
        group.item_id.clone(),
    )
}

pub(super) fn budget_error(message: impl Into<String>) -> ContextAssemblyError {
    ContextAssemblyError::new("context_budget_exceeded", message)
}
