use crate::llm::ir::LlmContentPartIr;

use super::{
    ContextAssemblyError, ContextAssemblyResult, ContextBudgetAction, ContextBudgetStrategy,
    ContextTokenCounter, OverflowPolicy, budget::budget_error, candidate::CandidateGroup,
};

pub(super) struct OverflowOutcome {
    pub included: bool,
    pub tokens: u64,
    pub action: ContextBudgetAction,
    pub reason: String,
}

pub(super) fn apply_overflow(
    group: &mut CandidateGroup,
    cap: u64,
    strategy: ContextBudgetStrategy,
    counter: &dyn ContextTokenCounter,
) -> ContextAssemblyResult<OverflowOutcome> {
    match group.overflow.as_ref() {
        Some(OverflowPolicy::Drop) => Ok(drop_group(group, "overflow policy drop")),
        Some(OverflowPolicy::TruncateHead) => {
            require_non_empty(truncate_group(group, cap, false, counter)?, strategy, group)
        }
        Some(OverflowPolicy::TruncateTail) => {
            require_non_empty(truncate_group(group, cap, true, counter)?, strategy, group)
        }
        Some(OverflowPolicy::KeepRecent { .. }) => {
            require_non_empty(keep_recent(group, cap)?, strategy, group)
        }
        Some(OverflowPolicy::TopK { .. } | OverflowPolicy::Dedupe) | None => match strategy {
            ContextBudgetStrategy::BestEffort => Ok(drop_group(group, "best_effort budget drop")),
            ContextBudgetStrategy::Strict => Err(budget_error(format!(
                "optional context item does not fit its budget: {}",
                group.item_id
            ))),
        },
    }
}

fn require_non_empty(
    outcome: OverflowOutcome,
    strategy: ContextBudgetStrategy,
    group: &CandidateGroup,
) -> ContextAssemblyResult<OverflowOutcome> {
    if !outcome.included && strategy == ContextBudgetStrategy::Strict {
        Err(budget_error(format!(
            "optional context item has no non-empty value that fits: {}",
            group.item_id
        )))
    } else {
        Ok(outcome)
    }
}

fn keep_recent(group: &mut CandidateGroup, cap: u64) -> ContextAssemblyResult<OverflowOutcome> {
    let mut used = 0u64;
    let mut first = group.candidates.len();
    for (index, candidate) in group.candidates.iter().enumerate().rev() {
        let next = used
            .checked_add(candidate.token_count)
            .ok_or_else(|| budget_error("history token count overflow"))?;
        if next > cap {
            break;
        }
        used = next;
        first = index;
    }
    group.candidates.drain(..first);
    for candidate in &mut group.candidates {
        candidate.included = true;
        if !candidate
            .provenance
            .transformations
            .iter()
            .any(|value| value == "keep_recent")
        {
            candidate
                .provenance
                .transformations
                .push("keep_recent".into());
        }
    }
    Ok(OverflowOutcome {
        included: !group.candidates.is_empty(),
        tokens: used,
        action: if group.candidates.is_empty() {
            ContextBudgetAction::Dropped
        } else {
            ContextBudgetAction::Truncated
        },
        reason: "history kept the newest fitting suffix".into(),
    })
}

fn truncate_group(
    group: &mut CandidateGroup,
    cap: u64,
    keep_head: bool,
    counter: &dyn ContextTokenCounter,
) -> ContextAssemblyResult<OverflowOutcome> {
    if group.candidates.len() != 1 {
        return Err(ContextAssemblyError::new(
            "context_truncate_shape_unsupported",
            "truncate overflow requires exactly one resolved value",
        ));
    }
    let candidate = &mut group.candidates[0];
    if candidate
        .content
        .iter()
        .any(|part| !matches!(part, LlmContentPartIr::Text { .. }))
    {
        return Err(ContextAssemblyError::new(
            "context_binary_truncate_forbidden",
            "image and file content cannot be truncated",
        ));
    }
    let scalar_count = candidate
        .content
        .iter()
        .map(|part| match part {
            LlmContentPartIr::Text { text } => text.chars().count(),
            _ => 0,
        })
        .sum::<usize>();
    let mut low = 0usize;
    let mut high = scalar_count;
    let mut best = None;
    while low <= high {
        let mid = low + (high - low) / 2;
        let content = keep_scalars(&candidate.content, mid, keep_head);
        let tokens = if content.is_empty() {
            0
        } else {
            counter.count(candidate.role, &content)?
        };
        if tokens <= cap {
            best = Some((content, tokens));
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    let Some((content, tokens)) = best.filter(|(content, _)| !content.is_empty()) else {
        return Ok(drop_group(group, "no non-empty truncation fits"));
    };
    candidate.content = content;
    candidate.token_count = tokens;
    candidate.included = true;
    candidate.provenance.transformations.push(if keep_head {
        "truncate_tail".into()
    } else {
        "truncate_head".into()
    });
    Ok(OverflowOutcome {
        included: true,
        tokens,
        action: ContextBudgetAction::Truncated,
        reason: "text was truncated at a Unicode scalar boundary".into(),
    })
}

fn keep_scalars(
    content: &[LlmContentPartIr],
    quota: usize,
    keep_head: bool,
) -> Vec<LlmContentPartIr> {
    if keep_head {
        let mut remaining = quota;
        content
            .iter()
            .filter_map(|part| match part {
                LlmContentPartIr::Text { text } if remaining > 0 => {
                    let kept: String = text.chars().take(remaining).collect();
                    remaining = remaining.saturating_sub(kept.chars().count());
                    (!kept.is_empty()).then_some(LlmContentPartIr::Text { text: kept })
                }
                _ => None,
            })
            .collect()
    } else {
        keep_tail_scalars(content, quota)
    }
}

fn keep_tail_scalars(content: &[LlmContentPartIr], quota: usize) -> Vec<LlmContentPartIr> {
    let total: usize = content
        .iter()
        .map(|part| match part {
            LlmContentPartIr::Text { text } => text.chars().count(),
            _ => 0,
        })
        .sum();
    let mut skip = total.saturating_sub(quota);
    let mut remaining = quota;
    let mut output = Vec::new();
    for part in content {
        let LlmContentPartIr::Text { text } = part else {
            continue;
        };
        let count = text.chars().count();
        if skip >= count {
            skip -= count;
            continue;
        }
        let kept: String = text.chars().skip(skip).take(remaining).collect();
        skip = 0;
        remaining = remaining.saturating_sub(kept.chars().count());
        if !kept.is_empty() {
            output.push(LlmContentPartIr::Text { text: kept });
        }
    }
    output
}

fn drop_group(group: &mut CandidateGroup, reason: &str) -> OverflowOutcome {
    for candidate in &mut group.candidates {
        candidate.included = false;
    }
    OverflowOutcome {
        included: false,
        tokens: 0,
        action: ContextBudgetAction::Dropped,
        reason: reason.into(),
    }
}
