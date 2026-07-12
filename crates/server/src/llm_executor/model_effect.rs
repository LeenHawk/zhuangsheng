use zhuangsheng_core::{
    graph::{EffectClassification, HostedToolBinding},
    llm::EffectRetryPolicy,
};

pub(super) struct ModelEffectProfile {
    pub kind: &'static str,
    pub classification: EffectClassification,
    pub operation_key: &'static str,
}

pub(super) fn model_effect(hosted_tools: &[HostedToolBinding]) -> ModelEffectProfile {
    let classification = hosted_tools
        .iter()
        .fold(EffectClassification::Idempotent, |current, hosted| {
            highest_effect(current, hosted.effect.classification)
        });
    ModelEffectProfile {
        kind: if hosted_tools.is_empty() {
            "model_generation"
        } else {
            "model_generation_hosted"
        },
        classification,
        operation_key: if hosted_tools.is_empty() {
            "llm.generate"
        } else {
            "llm.generate.hosted"
        },
    }
}

pub(super) fn model_retry_policy(classification: EffectClassification) -> EffectRetryPolicy {
    EffectRetryPolicy {
        max_attempts: if classification == EffectClassification::NonIdempotent {
            1
        } else {
            2
        },
        backoff_ms: if classification == EffectClassification::NonIdempotent {
            Vec::new()
        } else {
            vec![250]
        },
    }
}

fn highest_effect(left: EffectClassification, right: EffectClassification) -> EffectClassification {
    match (left, right) {
        (EffectClassification::NonIdempotent, _) | (_, EffectClassification::NonIdempotent) => {
            EffectClassification::NonIdempotent
        }
        (EffectClassification::Idempotent, _) | (_, EffectClassification::Idempotent) => {
            EffectClassification::Idempotent
        }
        _ => EffectClassification::Pure,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zhuangsheng_core::{
        graph::ToolEffectSpec,
        llm::{ContentGenerationKind, Operation, OperationKey},
    };

    use super::*;

    #[test]
    fn hosted_risk_controls_model_effect_retryability() {
        let mut binding = HostedToolBinding {
            binding_id: "search".into(),
            operation_key: OperationKey::content_generation(
                Operation::GenerateContent,
                ContentGenerationKind::OpenAiResponses,
            ),
            hosted_kind: "web_search".into(),
            model_facing_config: BTreeMap::new(),
            resource_scopes: vec!["internet:public".into()],
            effect: ToolEffectSpec {
                classification: EffectClassification::Pure,
                operation_key: "hosted.web_search".into(),
                requires_approval: false,
            },
            max_uses_per_model_call: 1,
        };
        assert_eq!(
            model_effect(&[binding.clone()]).classification,
            EffectClassification::Idempotent
        );
        binding.effect.classification = EffectClassification::NonIdempotent;
        let effect = model_effect(&[binding]);
        assert_eq!(effect.classification, EffectClassification::NonIdempotent);
        let retry = model_retry_policy(effect.classification);
        assert_eq!(retry.max_attempts, 1);
        assert!(retry.backoff_ms.is_empty());
    }
}
