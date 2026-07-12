use std::collections::BTreeMap;

use zhuangsheng_core::llm::{
    ResolvedHostedTool,
    ir::{HostedToolPhase, LlmStreamEventIr, LlmTurnItemIr},
};

#[derive(Debug)]
pub(super) struct HostedToolResponseError {
    pub code: &'static str,
    pub message: &'static str,
}

pub(super) fn bind_hosted_response_items(
    items: &mut [LlmTurnItemIr],
    resolved: &[ResolvedHostedTool],
) -> Result<(), HostedToolResponseError> {
    let mut by_provider_kind = BTreeMap::new();
    for tool in resolved {
        if by_provider_kind
            .insert(tool.provider_item_kind.as_str(), tool)
            .is_some()
        {
            return error(
                "ambiguous_hosted_tool_binding",
                "hosted provider item kind maps to multiple grants",
            );
        }
    }
    let mut uses = BTreeMap::<&str, u64>::new();
    for item in items {
        let LlmTurnItemIr::HostedTool {
            binding_id,
            kind,
            phase,
            ..
        } = item
        else {
            continue;
        };
        let tool = by_provider_kind.get(kind.as_str()).copied().or_else(|| {
            resolved.iter().find(|tool| {
                tool.binding.hosted_kind == *kind && tool.binding.binding_id == *binding_id
            })
        });
        let tool = tool.ok_or(HostedToolResponseError {
            code: "ungranted_hosted_tool_item",
            message: "provider returned a hosted item outside the granted capability envelope",
        })?;
        if !matches!(phase, HostedToolPhase::Completed | HostedToolPhase::Failed) {
            return error(
                "incomplete_hosted_tool_item",
                "provider terminal contains an unfinished hosted tool item",
            );
        }
        let used = uses.entry(tool.binding.binding_id.as_str()).or_default();
        *used = used.saturating_add(1);
        if *used > tool.binding.max_uses_per_model_call {
            return error(
                "hosted_tool_use_limit_exceeded",
                "provider exceeded the hosted tool per-model-call use limit",
            );
        }
        *binding_id = tool.binding.binding_id.clone();
        *kind = tool.binding.hosted_kind.clone();
    }
    Ok(())
}

pub(super) fn bind_hosted_stream_events(
    events: &mut [LlmStreamEventIr],
    resolved: &[ResolvedHostedTool],
) -> Result<(), HostedToolResponseError> {
    for event in events {
        match event {
            LlmStreamEventIr::HostedToolEvent { item, .. } => {
                bind_hosted_response_items(std::slice::from_mut(item), resolved)?;
            }
            LlmStreamEventIr::Completed { response, .. } => {
                bind_hosted_response_items(&mut response.items, resolved)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn error<T>(code: &'static str, message: &'static str) -> Result<T, HostedToolResponseError> {
    Err(HostedToolResponseError { code, message })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zhuangsheng_core::{
        graph::{EffectClassification, HostedToolBinding, ToolEffectSpec},
        llm::{ContentGenerationKind, Operation, OperationKey},
    };

    use super::*;

    #[test]
    fn binds_authorized_provider_kind_and_enforces_use_limit() {
        let resolved = vec![ResolvedHostedTool {
            binding: HostedToolBinding {
                binding_id: "search-binding".into(),
                operation_key: OperationKey::content_generation(
                    Operation::GenerateContent,
                    ContentGenerationKind::OpenAiResponses,
                ),
                hosted_kind: "web_search".into(),
                model_facing_config: BTreeMap::new(),
                resource_scopes: vec!["internet:public".into()],
                effect: ToolEffectSpec {
                    classification: EffectClassification::Idempotent,
                    operation_key: "hosted.web_search".into(),
                    requires_approval: false,
                },
                max_uses_per_model_call: 1,
            },
            provider_item_kind: "web_search_call".into(),
        }];
        let item = || LlmTurnItemIr::HostedTool {
            id: "hosted-1".into(),
            binding_id: "web_search_call".into(),
            kind: "web_search_call".into(),
            phase: HostedToolPhase::Completed,
            display_content: Vec::new(),
            opaque_item_ref: None,
        };
        let mut items = vec![item()];
        bind_hosted_response_items(&mut items, &resolved).unwrap();
        assert!(matches!(
            &items[0],
            LlmTurnItemIr::HostedTool { binding_id, kind, .. }
                if binding_id == "search-binding" && kind == "web_search"
        ));

        let mut over_limit = vec![item(), item()];
        assert_eq!(
            bind_hosted_response_items(&mut over_limit, &resolved)
                .unwrap_err()
                .code,
            "hosted_tool_use_limit_exceeded"
        );
        let mut unknown = vec![LlmTurnItemIr::HostedTool {
            id: "hosted-2".into(),
            binding_id: "file_search_call".into(),
            kind: "file_search_call".into(),
            phase: HostedToolPhase::Completed,
            display_content: Vec::new(),
            opaque_item_ref: None,
        }];
        assert_eq!(
            bind_hosted_response_items(&mut unknown, &resolved)
                .unwrap_err()
                .code,
            "ungranted_hosted_tool_item"
        );

        let mut events = vec![LlmStreamEventIr::HostedToolEvent {
            call_id: "call-1".into(),
            seq: 0,
            item: item(),
        }];
        bind_hosted_stream_events(&mut events, &resolved).unwrap();
        assert!(matches!(
            &events[0],
            LlmStreamEventIr::HostedToolEvent {
                item: LlmTurnItemIr::HostedTool { binding_id, kind, .. },
                ..
            } if binding_id == "search-binding" && kind == "web_search"
        ));
    }
}
