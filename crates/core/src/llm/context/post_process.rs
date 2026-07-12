use crate::llm::ir::{
    ContextProvenanceIr, ContextSensitivity, ContextTrust, LlmContentPartIr, MessageRole,
    ProvenanceRole,
};

use super::{
    AssembledMessageIr, ContextAssemblyError, ContextAssemblyResult, PromptPostProcessRule,
};

pub(super) fn apply_post_process(
    messages: &mut Vec<AssembledMessageIr>,
    provenance: &mut Vec<ContextProvenanceIr>,
    rules: &[PromptPostProcessRule],
) -> ContextAssemblyResult<()> {
    for rule in rules {
        match rule {
            PromptPostProcessRule::StripEmptyMessages => strip_empty(messages),
            PromptPostProcessRule::MergeAdjacentMessages => merge_adjacent(messages, provenance)?,
            PromptPostProcessRule::StrictAlternation => strict_alternation(messages, provenance),
            PromptPostProcessRule::SinglePrompt => single_prompt(messages, provenance)?,
        }
    }
    Ok(())
}

pub(super) fn single_prompt(
    messages: &mut Vec<AssembledMessageIr>,
    provenance: &mut Vec<ContextProvenanceIr>,
) -> ContextAssemblyResult<()> {
    messages.retain(|message| !is_placeholder(message, provenance));
    if messages.is_empty() {
        return Ok(());
    }
    if messages.len() == 1 && messages[0].role == MessageRole::User {
        return Ok(());
    }
    let mut refs = Vec::with_capacity(messages.len());
    for message in messages.iter() {
        refs.push(provenance_by_id(provenance, &message.provenance_id)?);
    }
    let has_public = refs
        .iter()
        .any(|value| value.sensitivity == ContextSensitivity::Public);
    let has_sensitive = refs
        .iter()
        .any(|value| value.sensitivity == ContextSensitivity::Sensitive);
    if has_public && has_sensitive {
        return Err(ContextAssemblyError::new(
            "context_post_process_sensitivity_boundary",
            "single_prompt cannot merge public and sensitive provenance",
        ));
    }
    let trust = refs
        .iter()
        .map(|value| value.trust)
        .max_by_key(|value| trust_rank(*value))
        .expect("non-empty provenance");
    let sensitivity = refs
        .iter()
        .map(|value| value.sensitivity)
        .max_by_key(|value| sensitivity_rank(*value))
        .expect("non-empty provenance");
    let mut content = Vec::new();
    for message in messages.iter_mut() {
        content.append(&mut message.content);
    }
    let message_id = messages[0].id.clone();
    let provenance_id = "ctxprov:single-prompt".to_owned();
    provenance.push(ContextProvenanceIr {
        id: provenance_id.clone(),
        item_id: "__single_prompt__".into(),
        source_type: "assembled_prompt".into(),
        source_id: "single_prompt".into(),
        trust,
        sensitivity,
        final_role: ProvenanceRole::User,
        transformations: vec!["single_prompt".into()],
    });
    messages.clear();
    messages.push(AssembledMessageIr {
        id: message_id,
        role: MessageRole::User,
        content,
        provenance_id,
        placeholder: false,
    });
    Ok(())
}

fn strip_empty(messages: &mut Vec<AssembledMessageIr>) {
    messages.retain(|message| !content_is_empty(&message.content));
}

fn merge_adjacent(
    messages: &mut Vec<AssembledMessageIr>,
    provenance: &mut [ContextProvenanceIr],
) -> ContextAssemblyResult<()> {
    let mut merged: Vec<AssembledMessageIr> = Vec::with_capacity(messages.len());
    for mut message in messages.drain(..) {
        let can_merge = if let Some(previous) = merged.last() {
            let left = provenance_by_id(provenance, &previous.provenance_id)?;
            let right = provenance_by_id(provenance, &message.provenance_id)?;
            previous.role == message.role
                && left.trust == right.trust
                && left.sensitivity == right.sensitivity
        } else {
            false
        };
        if can_merge {
            let previous = merged.last_mut().expect("checked above");
            previous.content.append(&mut message.content);
            let provenance_id = previous.provenance_id.clone();
            let entry = provenance
                .iter_mut()
                .find(|value| value.id == provenance_id)
                .expect("validated provenance reference");
            if !entry
                .transformations
                .iter()
                .any(|value| value == "merge_adjacent_messages")
            {
                entry.transformations.push("merge_adjacent_messages".into());
            }
        } else {
            merged.push(message);
        }
    }
    *messages = merged;
    Ok(())
}

fn strict_alternation(
    messages: &mut Vec<AssembledMessageIr>,
    provenance: &mut Vec<ContextProvenanceIr>,
) {
    let mut output = Vec::with_capacity(messages.len().saturating_mul(2));
    let mut ordinal = 0usize;
    for message in messages.drain(..) {
        let expected = output
            .last()
            .map_or(MessageRole::User, |previous: &AssembledMessageIr| {
                opposite(previous.role)
            });
        if message.role != expected {
            ordinal += 1;
            let provenance_id = format!("ctxprov:alternation:{ordinal}");
            provenance.push(ContextProvenanceIr {
                id: provenance_id.clone(),
                item_id: "__adapter_placeholder__".into(),
                source_type: "adapter_placeholder".into(),
                source_id: ordinal.to_string(),
                trust: ContextTrust::TrustedConfig,
                sensitivity: ContextSensitivity::Public,
                final_role: match expected {
                    MessageRole::User => ProvenanceRole::User,
                    MessageRole::Assistant => ProvenanceRole::Assistant,
                },
                transformations: vec!["strict_alternation_placeholder".into()],
            });
            output.push(AssembledMessageIr {
                id: format!("ctx-placeholder:{ordinal}"),
                role: expected,
                content: Vec::new(),
                provenance_id,
                placeholder: true,
            });
        }
        output.push(message);
    }
    *messages = output;
}

fn provenance_by_id<'a>(
    provenance: &'a [ContextProvenanceIr],
    id: &str,
) -> ContextAssemblyResult<&'a ContextProvenanceIr> {
    provenance
        .iter()
        .find(|value| value.id == id)
        .ok_or_else(|| {
            ContextAssemblyError::new(
                "context_provenance_missing",
                format!("assembled message references missing provenance: {id}"),
            )
        })
}

fn is_placeholder(message: &AssembledMessageIr, provenance: &[ContextProvenanceIr]) -> bool {
    message.placeholder
        && content_is_empty(&message.content)
        && provenance
            .iter()
            .find(|value| value.id == message.provenance_id)
            .is_some_and(|value| value.source_type == "adapter_placeholder")
}

fn content_is_empty(content: &[LlmContentPartIr]) -> bool {
    content.is_empty()
        || content.iter().all(|part| match part {
            LlmContentPartIr::Text { text } => text.is_empty(),
            LlmContentPartIr::Image { .. } | LlmContentPartIr::File { .. } => false,
        })
}

fn opposite(role: MessageRole) -> MessageRole {
    match role {
        MessageRole::User => MessageRole::Assistant,
        MessageRole::Assistant => MessageRole::User,
    }
}

fn trust_rank(value: ContextTrust) -> u8 {
    match value {
        ContextTrust::RuntimePolicy => 0,
        ContextTrust::TrustedConfig => 1,
        ContextTrust::UserInput => 2,
        ContextTrust::ExternalUntrusted => 3,
    }
}

fn sensitivity_rank(value: ContextSensitivity) -> u8 {
    match value {
        ContextSensitivity::Public => 0,
        ContextSensitivity::Private => 1,
        ContextSensitivity::Sensitive => 2,
    }
}
