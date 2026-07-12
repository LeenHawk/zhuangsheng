use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    graph::{AppliedGraphDefinition, DraftNodeKind},
    llm::context::{ContextAssemblyConfig, ContextAssemblySpec},
};

use super::{ConversationInputShape, ConversationRunSpec, validate_conversation_run_contract};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RolePlayCompatibilityView {
    Editable {
        profile_version: u32,
        editable_fields: Vec<String>,
    },
    Partial {
        profile_version: u32,
        editable_fields: Vec<String>,
        locked_reasons: Vec<String>,
    },
    ExpertOnly {
        reasons: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolePlayGraphOptionView {
    pub graph_id: String,
    pub graph_name: String,
    pub revision_id: String,
    pub revision_no: u64,
    pub reply_output_keys: Vec<String>,
    pub primary_llm_node_id: Option<String>,
    pub compatibility: RolePlayCompatibilityView,
}

pub struct RolePlayCompatibilityAnalysis {
    pub reply_output_keys: Vec<String>,
    pub primary_llm_node_id: Option<String>,
    pub compatibility: RolePlayCompatibilityView,
}

pub fn analyze_roleplay_compatibility(
    definition: &AppliedGraphDefinition,
    preset: Option<&ContextAssemblySpec>,
) -> RolePlayCompatibilityAnalysis {
    let reply_output_keys: Vec<_> = definition
        .output_contract
        .iter()
        .filter_map(|output| {
            let run = ConversationRunSpec {
                graph_revision_id: "compatibility-probe".into(),
                reply_output_key: output.key.clone(),
                input_shape: ConversationInputShape::ConversationMessageV1,
            };
            validate_conversation_run_contract(definition, &run)
                .is_ok()
                .then(|| output.key.clone())
        })
        .collect();
    if reply_output_keys.is_empty() {
        return expert_only("conversation_contract_incompatible", reply_output_keys);
    }
    let llm_nodes: Vec<_> = definition
        .nodes
        .iter()
        .filter(|node| matches!(&node.kind, DraftNodeKind::Llm { .. }))
        .collect();
    if llm_nodes.len() != 1 {
        return expert_only("primary_llm_node_not_unique", reply_output_keys);
    }
    let primary = llm_nodes[0];
    let DraftNodeKind::Llm { config } = &primary.kind else {
        unreachable!("filtered LLM node")
    };
    let mut editable = BTreeSet::from([
        "generation".to_owned(),
        "model".to_owned(),
        "streaming".to_owned(),
    ]);
    let mut locked = BTreeSet::new();
    if definition.nodes.iter().any(|node| {
        !matches!(
            &node.kind,
            DraftNodeKind::Input { .. } | DraftNodeKind::Output { .. } | DraftNodeKind::Llm { .. }
        )
    }) {
        locked.insert("custom_coordination_nodes".to_owned());
    }
    if !config.tools.is_empty() || !config.hosted_tools.is_empty() {
        locked.insert("tool_permissions_require_expert".to_owned());
    }
    if config.memory.is_some() {
        locked.insert("memory_binding_requires_expert".to_owned());
    }
    let context = match &config.context {
        ContextAssemblyConfig::Inline { spec } => Some(spec),
        ContextAssemblyConfig::Preset { .. } => preset,
    };
    match context {
        Some(spec) => analyze_context(spec, &mut editable, &mut locked),
        None => {
            locked.insert("context_preset_profile_unavailable".to_owned());
        }
    }
    let editable_fields = editable.into_iter().collect();
    let compatibility = if locked.is_empty() {
        RolePlayCompatibilityView::Editable {
            profile_version: 1,
            editable_fields,
        }
    } else {
        RolePlayCompatibilityView::Partial {
            profile_version: 1,
            editable_fields,
            locked_reasons: locked.into_iter().collect(),
        }
    };
    RolePlayCompatibilityAnalysis {
        reply_output_keys,
        primary_llm_node_id: Some(primary.id.clone()),
        compatibility,
    }
}

fn analyze_context(
    spec: &ContextAssemblySpec,
    editable: &mut BTreeSet<String>,
    locked: &mut BTreeSet<String>,
) {
    const KNOWN: &[&str] = &[
        "character",
        "persona",
        "world",
        "lore",
        "history",
        "summary",
        "style",
    ];
    for item in &spec.items {
        if item.id == "input"
            && matches!(item.requested_role, crate::llm::context::ContextRole::User)
            && matches!(
                item.position,
                crate::llm::context::ContextPosition::UserInput
            )
            && matches!(
                &item.source,
                crate::llm::context::ContextSource::Input { path } if path == "/content"
            )
        {
            continue;
        }
        let profile = item.id.split([':', '/']).next().unwrap_or(&item.id);
        if KNOWN.contains(&profile) {
            editable.insert(format!("context.{profile}"));
        } else {
            locked.insert("unknown_context_items".to_owned());
        }
    }
}

fn expert_only(reason: &str, reply_output_keys: Vec<String>) -> RolePlayCompatibilityAnalysis {
    RolePlayCompatibilityAnalysis {
        reply_output_keys,
        primary_llm_node_id: None,
        compatibility: RolePlayCompatibilityView::ExpertOnly {
            reasons: vec![reason.into()],
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::RolePlayCompatibilityView;

    #[test]
    fn compatibility_view_uses_the_versioned_camel_case_wire_shape() {
        let value = serde_json::to_value(RolePlayCompatibilityView::Editable {
            profile_version: 1,
            editable_fields: vec!["model".into()],
        })
        .unwrap();
        assert_eq!(
            value,
            json!({"mode":"editable","profileVersion":1,"editableFields":["model"]})
        );
    }
}
