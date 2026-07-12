use std::collections::HashSet;

use crate::{
    llm::{LlmConfigError, LlmConfigResult},
    selector,
};

use super::{template::compile_template, types::*};

const MAX_CONTEXT_ITEMS: usize = 256;
const MAX_ITEM_TEXT_BYTES: usize = 1024 * 1024;
const MAX_SELECTOR_LIMIT: u32 = 10_000;
const DEFAULT_ARTIFACT_MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextNormalizationPolicy {
    pub semantic_policy_version: u32,
    pub authorized_event_types: Vec<String>,
    pub default_artifact_max_bytes: u64,
}

impl Default for ContextNormalizationPolicy {
    fn default() -> Self {
        Self {
            semantic_policy_version: CONTEXT_SEMANTIC_POLICY_VERSION,
            authorized_event_types: vec![
                "node.completed".into(),
                "node.failed".into(),
                "run.completed".into(),
                "run.failed".into(),
                "run.waiting".into(),
            ],
            default_artifact_max_bytes: DEFAULT_ARTIFACT_MAX_BYTES,
        }
    }
}

pub fn normalize_context_spec(
    mut spec: ContextAssemblySpec,
    policy: &ContextNormalizationPolicy,
) -> LlmConfigResult<ContextAssemblySpec> {
    if policy.semantic_policy_version != CONTEXT_SEMANTIC_POLICY_VERSION {
        return Err(LlmConfigError::new(
            "unsupported_context_semantic_policy",
            "context semantic policy version is not supported",
        ));
    }
    normalize_optional_label(&mut spec.id, 128, "context spec id")?;
    normalize_optional_label(&mut spec.name, 200, "context spec name")?;
    if spec.items.len() > MAX_CONTEXT_ITEMS {
        return Err(LlmConfigError::new(
            "context_item_limit",
            "context preset contains more than 256 items",
        ));
    }
    let mut ids = HashSet::new();
    for item in &mut spec.items {
        normalize_item(item, policy)?;
        if !ids.insert(item.id.clone()) {
            return Err(LlmConfigError::new(
                "duplicate_context_item_id",
                "context item ids must be unique",
            ));
        }
    }
    let budget = spec.budget.get_or_insert(ContextBudgetPolicy {
        max_input_tokens: None,
        strategy: None,
    });
    budget.strategy.get_or_insert(ContextBudgetStrategy::Strict);
    if budget.max_input_tokens == Some(0) {
        return Err(LlmConfigError::new(
            "invalid_context_budget",
            "max input tokens must be positive when present",
        ));
    }
    validate_post_process(&spec.post_process)?;
    spec.preview.get_or_insert(PreviewPolicy {
        content: PreviewContent::MetadataOnly,
        count: PreviewCount::Local,
    });
    Ok(spec)
}

fn normalize_item(
    item: &mut ContextItem,
    policy: &ContextNormalizationPolicy,
) -> LlmConfigResult<()> {
    item.id = item.id.trim().to_owned();
    if item.id.is_empty() || item.id.len() > 128 {
        return Err(LlmConfigError::new(
            "invalid_context_item_id",
            "context item id must contain 1..=128 bytes",
        ));
    }
    normalize_optional_label(&mut item.name, 200, "context item name")?;
    if item.budget.max_tokens == Some(0) {
        return Err(LlmConfigError::new(
            "invalid_context_item_budget",
            "item max tokens must be positive when present",
        ));
    }
    if item.budget.required && item.overflow.is_some() {
        return Err(LlmConfigError::new(
            "required_context_item_overflow",
            "required context items cannot have an overflow transformation",
        ));
    }
    normalize_source(&mut item.source, policy)?;
    if !item.budget.required && item.overflow.is_none() {
        item.overflow = Some(if matches!(item.source, ContextSource::History { .. }) {
            OverflowPolicy::KeepRecent { count: None }
        } else {
            OverflowPolicy::Drop
        });
    }
    validate_source_position(item)?;
    validate_overflow(item)?;
    Ok(())
}

fn normalize_source(
    source: &mut ContextSource,
    policy: &ContextNormalizationPolicy,
) -> LlmConfigResult<()> {
    if let Some(binding_id) = source.binding_id()
        && (binding_id.trim().is_empty() || binding_id.len() > 128)
    {
        return Err(LlmConfigError::new(
            "invalid_context_binding",
            "binding id must contain 1..=128 bytes",
        ));
    }
    match source {
        ContextSource::Literal { text } => bounded_text(text),
        ContextSource::Template {
            template,
            variables,
            compiled,
            ..
        } => {
            validate_variable_sources(variables)?;
            *compiled = Some(compile_template(template, variables)?);
            Ok(())
        }
        ContextSource::Input { path } => validate_pointer(path),
        ContextSource::Memory { view, .. } => {
            view.get_or_insert(MemoryView::Summary);
            Ok(())
        }
        ContextSource::WorkingMemory { path, .. } | ContextSource::State { path, .. } => {
            let path = path.get_or_insert_with(String::new);
            validate_pointer(path)
        }
        ContextSource::History { strategy, .. } => match strategy {
            HistoryStrategy::All => Ok(()),
            HistoryStrategy::Recent { count } if *count > 0 && *count <= MAX_SELECTOR_LIMIT => {
                Ok(())
            }
            HistoryStrategy::Recent { .. } => bounded_selector_error(),
        },
        ContextSource::WorldInfo { selector, .. } => normalize_world_info(selector),
        ContextSource::Summary { scope, .. } => {
            scope.get_or_insert_with(String::new);
            Ok(())
        }
        ContextSource::ToolTrace { selector, .. } => {
            if selector.max_calls == 0 || selector.max_calls > MAX_SELECTOR_LIMIT {
                bounded_selector_error()
            } else {
                Ok(())
            }
        }
        ContextSource::EventTrace { selector, .. } => normalize_event_selector(selector, policy),
        ContextSource::Artifact { selector, .. } => {
            let selector = selector.get_or_insert(ArtifactSelector {
                view: ArtifactView::Metadata,
                max_bytes: policy.default_artifact_max_bytes,
            });
            if selector.max_bytes == 0 || selector.max_bytes > 16 * 1024 * 1024 {
                return Err(LlmConfigError::new(
                    "invalid_artifact_selector",
                    "artifact max bytes must be within 1..=16 MiB",
                ));
            }
            Ok(())
        }
        ContextSource::BranchContext { .. } => Ok(()),
    }
}

fn validate_variable_sources(
    variables: &std::collections::BTreeMap<String, TemplateVariableSource>,
) -> LlmConfigResult<()> {
    if variables.len() > 128 {
        return Err(LlmConfigError::new(
            "context_template_variable_limit",
            "template has more than 128 variables",
        ));
    }
    for source in variables.values() {
        match source {
            TemplateVariableSource::Literal { value } => {
                crate::canonical::to_vec(value).map_err(|error| {
                    LlmConfigError::new("invalid_template_literal", error.to_string())
                })?;
            }
            TemplateVariableSource::Input { selector }
            | TemplateVariableSource::Binding { selector, .. } => selector::validate(selector)
                .map_err(|message| LlmConfigError::new("invalid_template_selector", message))?,
        }
        if let TemplateVariableSource::Binding { binding_id, .. } = source
            && (binding_id.trim().is_empty() || binding_id.len() > 128)
        {
            return Err(LlmConfigError::new(
                "invalid_context_binding",
                "template binding id must contain 1..=128 bytes",
            ));
        }
    }
    Ok(())
}

fn normalize_world_info(selector: &mut WorldInfoSelector) -> LlmConfigResult<()> {
    if let WorldInfoSelector::Tags { tags, .. } = selector {
        if tags.is_empty() || tags.len() > 128 {
            return bounded_selector_error();
        }
        for tag in tags.iter_mut() {
            *tag = tag.trim().to_lowercase();
            if tag.is_empty() || tag.len() > 64 {
                return bounded_selector_error();
            }
        }
        tags.sort();
        tags.dedup();
    }
    Ok(())
}

fn normalize_event_selector(
    selector: &mut EventTraceSelector,
    policy: &ContextNormalizationPolicy,
) -> LlmConfigResult<()> {
    if selector.limit == 0 || selector.limit > MAX_SELECTOR_LIMIT {
        return bounded_selector_error();
    }
    selector.after_durable_seq.get_or_insert(0);
    let event_types = selector
        .event_types
        .get_or_insert_with(|| policy.authorized_event_types.clone());
    event_types.sort();
    event_types.dedup();
    if event_types.is_empty()
        || event_types
            .iter()
            .any(|event| !policy.authorized_event_types.contains(event))
    {
        return Err(LlmConfigError::new(
            "unauthorized_context_event_type",
            "event selector contains an unauthorized event type",
        ));
    }
    Ok(())
}

fn validate_source_position(item: &ContextItem) -> LlmConfigResult<()> {
    if matches!(item.source, ContextSource::History { .. })
        != matches!(item.position, ContextPosition::History)
    {
        return Err(LlmConfigError::new(
            "context_history_position_mismatch",
            "history sources must use the history position and vice versa",
        ));
    }
    if matches!(item.position, ContextPosition::AssistantPrefill)
        && !matches!(
            item.source,
            ContextSource::Literal { .. } | ContextSource::Template { .. }
        )
    {
        return Err(LlmConfigError::new(
            "untrusted_assistant_prefill",
            "assistant prefill must come from trusted literal or template configuration",
        ));
    }
    if matches!(item.position, ContextPosition::AssistantPrefill)
        && item.requested_role != ContextRole::Assistant
    {
        return Err(LlmConfigError::new(
            "invalid_assistant_prefill_role",
            "assistant prefill must request the assistant role",
        ));
    }
    Ok(())
}

fn validate_overflow(item: &ContextItem) -> LlmConfigResult<()> {
    match (&item.source, &item.overflow) {
        (ContextSource::History { .. }, Some(OverflowPolicy::KeepRecent { .. }))
        | (_, None)
        | (
            _,
            Some(
                OverflowPolicy::Drop
                | OverflowPolicy::TruncateHead
                | OverflowPolicy::TruncateTail
                | OverflowPolicy::Dedupe,
            ),
        ) => Ok(()),
        (ContextSource::History { .. }, _) => Err(LlmConfigError::new(
            "invalid_history_overflow",
            "history only supports keep_recent overflow",
        )),
        (_, Some(OverflowPolicy::KeepRecent { .. })) => Err(LlmConfigError::new(
            "invalid_keep_recent_overflow",
            "keep_recent is reserved for history sources",
        )),
        (_, Some(OverflowPolicy::TopK { k })) if *k > 0 && *k <= MAX_SELECTOR_LIMIT => Ok(()),
        (_, Some(OverflowPolicy::TopK { .. })) => bounded_selector_error(),
    }
}

fn validate_post_process(rules: &[PromptPostProcessRule]) -> LlmConfigResult<()> {
    let mut seen = HashSet::new();
    for rule in rules {
        let key = *rule as u8;
        if !seen.insert(key) {
            return Err(LlmConfigError::new(
                "duplicate_context_post_process",
                "post-process rules must be unique",
            ));
        }
    }
    Ok(())
}

fn normalize_optional_label(
    value: &mut Option<String>,
    max: usize,
    field: &'static str,
) -> LlmConfigResult<()> {
    *value = value
        .take()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if value.as_ref().is_some_and(|value| value.len() > max) {
        return Err(LlmConfigError::new(
            "context_label_limit",
            format!("{field} exceeds {max} bytes"),
        ));
    }
    Ok(())
}

fn bounded_text(text: &str) -> LlmConfigResult<()> {
    if text.len() > MAX_ITEM_TEXT_BYTES {
        Err(LlmConfigError::new(
            "context_text_limit",
            "context text exceeds one MiB",
        ))
    } else {
        Ok(())
    }
}

fn validate_pointer(pointer: &str) -> LlmConfigResult<()> {
    selector::validate(&crate::graph::InputSelector::JsonPointer {
        pointer: pointer.into(),
    })
    .map_err(|message| LlmConfigError::new("invalid_context_pointer", message))
}

fn bounded_selector_error<T>() -> LlmConfigResult<T> {
    Err(LlmConfigError::new(
        "invalid_context_selector_limit",
        "context selector is empty or exceeds the phase-one bound",
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn normalization_materializes_defaults_and_template_program() {
        let spec = ContextAssemblySpec {
            id: None,
            name: Some(" RP ".into()),
            mode: ContextAssemblyMode::Chat,
            items: vec![ContextItem {
                id: "character".into(),
                name: None,
                enabled: true,
                requested_role: ContextRole::System,
                source: ContextSource::Template {
                    syntax: TemplateSyntax::ZhuangshengTemplateV1,
                    template: "Character: {{name}}".into(),
                    variables: BTreeMap::from([(
                        "name".into(),
                        TemplateVariableSource::Literal {
                            value: serde_json::json!("Alice"),
                        },
                    )]),
                    on_missing: TemplateMissingPolicy::Error,
                    compiled: None,
                },
                position: ContextPosition::Start,
                order: 0,
                priority: 0,
                insertion_depth: 0,
                budget: TokenBudgetHint::default(),
                overflow: None,
            }],
            budget: None,
            post_process: vec![],
            preview: None,
        };
        let normalized =
            normalize_context_spec(spec, &ContextNormalizationPolicy::default()).unwrap();
        assert_eq!(normalized.name.as_deref(), Some("RP"));
        assert!(normalized.budget.unwrap().strategy.is_some());
        assert!(normalized.items[0].overflow.is_some());
        let ContextSource::Template { compiled, .. } = &normalized.items[0].source else {
            panic!("expected template")
        };
        assert!(compiled.is_some());
    }
}
