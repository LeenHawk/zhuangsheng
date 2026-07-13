use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{
    canonical,
    llm::ir::{ContextSensitivity, ContextTrust, InstructionIr, LlmContentPartIr, MessageRole},
};

use super::*;

pub(super) struct ScalarCounter;

impl ContextTokenCounter for ScalarCounter {
    fn count(
        &self,
        _role: ContextRole,
        content: &[LlmContentPartIr],
    ) -> ContextAssemblyResult<u64> {
        Ok(content
            .iter()
            .map(|part| match part {
                LlmContentPartIr::Text { text } => text.chars().count() as u64,
                LlmContentPartIr::Image { .. } | LlmContentPartIr::File { .. } => 1,
            })
            .sum())
    }
}

pub(super) fn spec(items: Vec<ContextItem>) -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items,
        budget: Some(ContextBudgetPolicy {
            max_input_tokens: None,
            strategy: Some(ContextBudgetStrategy::Strict),
        }),
        post_process: Vec::new(),
        text_transforms: Vec::new(),
        text_transform_macros: Default::default(),
        preview: None,
    }
}

pub(super) fn item(
    id: &str,
    role: ContextRole,
    source: ContextSource,
    position: ContextPosition,
    required: bool,
    priority: i64,
    overflow: Option<OverflowPolicy>,
) -> ContextItem {
    ContextItem {
        id: id.into(),
        name: None,
        enabled: true,
        requested_role: role,
        source,
        position,
        order: 0,
        priority,
        insertion_depth: 0,
        budget: TokenBudgetHint {
            max_tokens: None,
            required,
        },
        overflow,
    }
}

pub(super) fn input_item(id: &str, path: &str, order: i64) -> ContextItem {
    let mut value = item(
        id,
        ContextRole::User,
        ContextSource::Input { path: path.into() },
        ContextPosition::UserInput,
        false,
        0,
        Some(OverflowPolicy::Drop),
    );
    value.order = order;
    value
}

pub(super) fn assembly_input(
    spec: ContextAssemblySpec,
    node_input: Value,
    bindings: BTreeMap<String, ResolvedContextBinding>,
    context_window_tokens: u64,
) -> ContextAssemblyInput {
    let policy = ContextNormalizationPolicy::default();
    let spec = normalize_context_spec(spec, &policy).unwrap();
    let content_hash = canonical::hash(
        &json!({"semanticPolicyVersion": policy.semantic_policy_version, "spec": &spec}),
    )
    .unwrap();
    ContextAssemblyInput {
        node_input,
        config: ContextConfigSnapshot::GraphInline {
            graph_revision_id: "graphrev:test".into(),
            node_id: "node:test".into(),
            content_hash,
            semantic_policy_version: policy.semantic_policy_version,
            spec,
        },
        bindings,
        budget: ContextBudgetInput {
            context_window_tokens,
            reserved_output_tokens: 0,
            fixed_request_tokens: 0,
            safety_margin_tokens: 0,
            count_source: ContextCountSource::Local,
        },
        read_set_ref: "readset:test".into(),
        read_set_digest: canonical::hash(&json!({})).unwrap(),
        allow_sensitive: false,
    }
}

pub(super) fn data_binding(id: &str, values: Vec<ResolvedContextValue>) -> ResolvedContextBinding {
    ResolvedContextBinding {
        binding_id: id.into(),
        scope: format!("memory:{id}"),
        version: "v1".into(),
        values,
        template_value: None,
        template_provenance: None,
    }
}

pub(super) fn data_value(
    id: &str,
    text: &str,
    trust: ContextTrust,
    sensitivity: ContextSensitivity,
    allowed_roles: Vec<ContextRole>,
    relevance_score_micros: Option<i64>,
) -> ResolvedContextValue {
    let content = vec![LlmContentPartIr::Text { text: text.into() }];
    ResolvedContextValue::Data {
        id: id.into(),
        content_hash: canonical::hash(&content).unwrap(),
        content,
        provenance: ContextProvenance {
            source_type: "test".into(),
            source_id: id.into(),
            trust,
            sensitivity,
        },
        allowed_roles,
        relevance_score_micros,
        tags: Vec::new(),
    }
}

pub(super) fn history_value(
    id: &str,
    stable_order: u64,
    role: MessageRole,
    text: &str,
) -> ResolvedContextValue {
    let content = vec![LlmContentPartIr::Text { text: text.into() }];
    ResolvedContextValue::HistoryMessage {
        message_id: id.into(),
        turn_id: format!("turn:{id}"),
        stable_order,
        role,
        content_hash: canonical::hash(&content).unwrap(),
        content,
        provenance: ContextProvenance {
            source_type: "history".into(),
            source_id: id.into(),
            trust: ContextTrust::ExternalUntrusted,
            sensitivity: ContextSensitivity::Private,
        },
    }
}

pub(super) fn truncation_output(overflow: OverflowPolicy) -> ContextAssemblyOutput {
    let spec = spec(vec![item(
        "text",
        ContextRole::Context,
        ContextSource::Literal {
            text: "甲乙丙丁".into(),
        },
        ContextPosition::Start,
        false,
        0,
        Some(overflow),
    )]);
    assemble_context(
        &assembly_input(spec, json!(null), BTreeMap::new(), 2),
        &ScalarCounter,
    )
    .unwrap()
}

pub(super) fn instruction_text(value: &InstructionIr) -> &str {
    match &value.content[0] {
        LlmContentPartIr::Text { text } => text,
        _ => panic!("expected text"),
    }
}

pub(super) fn message_text(value: &AssembledMessageIr) -> String {
    value
        .content
        .iter()
        .filter_map(|part| match part {
            LlmContentPartIr::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
