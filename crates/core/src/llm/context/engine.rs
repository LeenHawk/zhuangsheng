use std::collections::BTreeSet;

use serde_json::json;

use crate::{
    canonical,
    llm::ir::{
        ContextSensitivity, ContextTrust, InstructionIr, InstructionRole, MessageRole,
        ProvenanceRole,
    },
};

use super::{
    AssembledMessageIr, CONTEXT_SEMANTIC_POLICY_VERSION, ContextAssemblyError,
    ContextAssemblyInput, ContextAssemblyMode, ContextAssemblyOutput, ContextAssemblyResult,
    ContextAssemblySnapshot, ContextAssemblySnapshotConfig, ContextAssemblySpec,
    ContextConfigSnapshot, ContextNormalizationPolicy, ContextPosition, ContextRole,
    ContextTokenCounter,
    budget::apply_budget,
    candidate::{CandidateGroup, ContextCandidate},
    normalize_context_spec,
    post_process::{apply_post_process, single_prompt},
    resolve::resolve_items,
};

pub fn assemble_context(
    input: &ContextAssemblyInput,
    counter: &dyn ContextTokenCounter,
) -> ContextAssemblyResult<ContextAssemblyOutput> {
    validate_read_set(input)?;
    let (spec, snapshot_config) = validate_config(&input.config)?;
    validate_bindings(input)?;
    let resolved_bindings_digest = canonical::hash(&input.bindings)?;
    let mut groups = resolve_items(input, &spec.items)?;
    validate_candidates(input, &groups)?;
    let history_len = groups
        .iter()
        .filter(|group| group.position == ContextPosition::History)
        .try_fold(0usize, |total, group| {
            total.checked_add(group.candidates.len()).ok_or_else(|| {
                ContextAssemblyError::new(
                    "context_history_limit",
                    "resolved history length overflowed",
                )
            })
        })?;
    validate_insertion_depth(&groups, history_len)?;
    let policy = spec.budget.as_ref().ok_or_else(|| {
        ContextAssemblyError::new(
            "context_snapshot_not_normalized",
            "context snapshot has no normalized budget policy",
        )
    })?;
    let budget_report = apply_budget(&mut groups, &input.budget, policy, counter)?;
    let ordered = ordered_candidates(groups, history_len);
    let (instructions, mut messages, mut provenance) = compile_candidates(ordered)?;
    apply_post_process(&mut messages, &mut provenance, &spec.post_process)?;
    if spec.mode == ContextAssemblyMode::Completion {
        single_prompt(&mut messages, &mut provenance)?;
    }
    validate_output_ids(&instructions, &messages, &provenance)?;
    let assembly_digest = canonical::hash(&json!({
        "config": &snapshot_config,
        "readSetRef": &input.read_set_ref,
        "readSetDigest": &input.read_set_digest,
        "resolvedBindingsDigest": &resolved_bindings_digest,
        "instructions": &instructions,
        "messages": &messages,
        "provenance": &provenance,
        "budgetReport": &budget_report,
    }))?;
    Ok(ContextAssemblyOutput {
        instructions,
        messages,
        provenance,
        budget_report,
        snapshot: ContextAssemblySnapshot {
            config: snapshot_config,
            read_set_ref: input.read_set_ref.clone(),
            read_set_digest: input.read_set_digest.clone(),
            resolved_bindings_digest,
            assembly_digest,
        },
    })
}

fn validate_read_set(input: &ContextAssemblyInput) -> ContextAssemblyResult<()> {
    if input.read_set_ref.trim().is_empty() || input.read_set_digest.trim().is_empty() {
        Err(ContextAssemblyError::new(
            "context_read_set_missing",
            "context assembly requires a pinned read-set reference and digest",
        ))
    } else {
        Ok(())
    }
}

fn validate_config(
    config: &ContextConfigSnapshot,
) -> ContextAssemblyResult<(ContextAssemblySpec, ContextAssemblySnapshotConfig)> {
    let (spec, semantic_policy_version, content_hash, snapshot) = match config {
        ContextConfigSnapshot::Preset {
            preset_id,
            version_id,
            version,
            content_hash,
            semantic_policy_version,
            spec,
        } => (
            spec,
            *semantic_policy_version,
            content_hash,
            ContextAssemblySnapshotConfig::Preset {
                preset_id: preset_id.clone(),
                version_id: version_id.clone(),
                version: *version,
                content_hash: content_hash.clone(),
            },
        ),
        ContextConfigSnapshot::GraphInline {
            graph_revision_id,
            node_id,
            content_hash,
            semantic_policy_version,
            spec,
        } => (
            spec,
            *semantic_policy_version,
            content_hash,
            ContextAssemblySnapshotConfig::GraphInline {
                graph_revision_id: graph_revision_id.clone(),
                node_id: node_id.clone(),
                content_hash: content_hash.clone(),
            },
        ),
    };
    if semantic_policy_version != CONTEXT_SEMANTIC_POLICY_VERSION {
        return Err(ContextAssemblyError::new(
            "unsupported_context_semantic_policy",
            "context snapshot uses an unsupported semantic policy",
        ));
    }
    let normalized = normalize_context_spec(spec.clone(), &ContextNormalizationPolicy::default())
        .map_err(|error| ContextAssemblyError::new(error.code, error.message))?;
    if &normalized != spec {
        return Err(ContextAssemblyError::new(
            "context_snapshot_not_normalized",
            "context snapshot is not fully normalized",
        ));
    }
    let expected_hash = canonical::hash(&json!({
        "semanticPolicyVersion": semantic_policy_version,
        "spec": spec,
    }))?;
    if content_hash != &expected_hash {
        return Err(ContextAssemblyError::new(
            "context_snapshot_hash_mismatch",
            "context snapshot content hash does not match its normalized spec",
        ));
    }
    Ok((spec.clone(), snapshot))
}

fn validate_bindings(input: &ContextAssemblyInput) -> ContextAssemblyResult<()> {
    for (key, binding) in &input.bindings {
        if key != &binding.binding_id
            || binding.binding_id.trim().is_empty()
            || binding.scope.trim().is_empty()
            || binding.version.trim().is_empty()
        {
            return Err(ContextAssemblyError::new(
                "context_binding_invalid",
                format!("resolved binding identity is invalid: {key}"),
            ));
        }
    }
    Ok(())
}

fn validate_candidates(
    input: &ContextAssemblyInput,
    groups: &[CandidateGroup],
) -> ContextAssemblyResult<()> {
    for group in groups {
        for candidate in &group.candidates {
            if candidate.provenance.sensitivity == ContextSensitivity::Sensitive
                && !input.allow_sensitive
            {
                return Err(ContextAssemblyError::new(
                    "context_sensitive_not_allowed",
                    format!(
                        "sensitive context is not allowed for item {}",
                        group.item_id
                    ),
                ));
            }
            if group.position == ContextPosition::AssistantPrefill
                && (candidate.role != ContextRole::Assistant
                    || !matches!(
                        candidate.provenance.trust,
                        ContextTrust::RuntimePolicy | ContextTrust::TrustedConfig
                    ))
            {
                return Err(ContextAssemblyError::new(
                    "context_assistant_prefill_unauthorized",
                    "assistant prefill must remain trusted assistant content",
                ));
            }
        }
    }
    Ok(())
}

fn validate_insertion_depth(
    groups: &[CandidateGroup],
    history_len: usize,
) -> ContextAssemblyResult<()> {
    for group in groups {
        let depth = usize::try_from(group.insertion_depth).unwrap_or(usize::MAX);
        if depth > history_len {
            return Err(ContextAssemblyError::new(
                "context_insertion_depth_out_of_range",
                format!("insertionDepth exceeds resolved history: {}", group.item_id),
            ));
        }
    }
    Ok(())
}

struct OrderedCandidate {
    rank: u8,
    anchor: u64,
    order: i64,
    item_index: usize,
    item_id: String,
    candidate: ContextCandidate,
}

fn ordered_candidates(groups: Vec<CandidateGroup>, history_len: usize) -> Vec<ContextCandidate> {
    let mut ordered = Vec::new();
    for group in groups {
        let anchor = u64::try_from(history_len.saturating_sub(group.insertion_depth as usize))
            .unwrap_or(u64::MAX);
        for candidate in group.candidates.into_iter().filter(|value| value.included) {
            ordered.push(OrderedCandidate {
                rank: position_rank(group.position),
                anchor: candidate.history_order.unwrap_or(anchor),
                order: group.order,
                item_index: group.item_index,
                item_id: group.item_id.clone(),
                candidate,
            });
        }
    }
    ordered.sort_by(|left, right| {
        (
            left.rank,
            left.anchor,
            left.order,
            left.item_index,
            &left.item_id,
        )
            .cmp(&(
                right.rank,
                right.anchor,
                right.order,
                right.item_index,
                &right.item_id,
            ))
            .then_with(|| left.candidate.sub_index.cmp(&right.candidate.sub_index))
            .then_with(|| left.candidate.id.cmp(&right.candidate.id))
    });
    ordered.into_iter().map(|value| value.candidate).collect()
}

fn compile_candidates(
    candidates: Vec<ContextCandidate>,
) -> ContextAssemblyResult<(
    Vec<InstructionIr>,
    Vec<AssembledMessageIr>,
    Vec<crate::llm::ir::ContextProvenanceIr>,
)> {
    let mut instructions = Vec::new();
    let mut messages = Vec::new();
    let mut provenance = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let provenance_id = candidate.provenance.id.clone();
        provenance.push(candidate.provenance.clone());
        match candidate.role {
            ContextRole::Policy
            | ContextRole::System
            | ContextRole::Developer
            | ContextRole::Context => instructions.push(InstructionIr {
                id: candidate.id,
                role: instruction_role(candidate.role),
                content: candidate.content,
                provenance: candidate.provenance,
            }),
            ContextRole::User | ContextRole::Assistant => messages.push(AssembledMessageIr {
                id: candidate.id,
                role: if candidate.role == ContextRole::User {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                content: candidate.content,
                provenance_id,
                placeholder: false,
            }),
        }
    }
    Ok((instructions, messages, provenance))
}

fn instruction_role(role: ContextRole) -> InstructionRole {
    match role {
        ContextRole::Policy => InstructionRole::Policy,
        ContextRole::System => InstructionRole::System,
        ContextRole::Developer => InstructionRole::Developer,
        ContextRole::Context => InstructionRole::Context,
        ContextRole::User | ContextRole::Assistant => unreachable!(),
    }
}

fn validate_output_ids(
    instructions: &[InstructionIr],
    messages: &[AssembledMessageIr],
    provenance: &[crate::llm::ir::ContextProvenanceIr],
) -> ContextAssemblyResult<()> {
    let mut ids = BTreeSet::new();
    for id in instructions
        .iter()
        .map(|value| value.id.as_str())
        .chain(messages.iter().map(|value| value.id.as_str()))
    {
        if id.is_empty() || !ids.insert(id) {
            return Err(ContextAssemblyError::new(
                "context_output_id_duplicate",
                "assembled instruction/message ids must be non-empty and unique",
            ));
        }
    }
    let provenance_ids: BTreeSet<_> = provenance.iter().map(|value| value.id.as_str()).collect();
    if provenance_ids.len() != provenance.len()
        || instructions
            .iter()
            .any(|value| !provenance_ids.contains(value.provenance.id.as_str()))
        || messages
            .iter()
            .any(|value| !provenance_ids.contains(value.provenance_id.as_str()))
        || provenance.iter().any(|value| {
            matches!(
                value.final_role,
                ProvenanceRole::User | ProvenanceRole::Assistant
            ) && value.id.is_empty()
        })
    {
        return Err(ContextAssemblyError::new(
            "context_provenance_invalid",
            "assembled provenance ids are missing, duplicated, or unresolved",
        ));
    }
    Ok(())
}

fn position_rank(position: ContextPosition) -> u8 {
    match position {
        ContextPosition::Start => 0,
        ContextPosition::BeforeHistory => 1,
        ContextPosition::History => 2,
        ContextPosition::AfterHistory => 3,
        ContextPosition::BeforeUserInput => 4,
        ContextPosition::UserInput => 5,
        ContextPosition::AssistantPrefill => 6,
        ContextPosition::End => 7,
    }
}
