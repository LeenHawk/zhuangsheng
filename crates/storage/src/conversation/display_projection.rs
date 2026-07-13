use std::collections::{BTreeMap, HashMap};

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    conversation::ConversationMessageRole,
    graph::LlmNodeExecutionSnapshot,
    llm::{
        context::ContextConfigSnapshot,
        ir::LlmContentPartIr,
        text_transform::{
            TextTransformContext, TextTransformRule, TextTransformSurface, TextTransformTarget,
            apply_text_transforms,
        },
    },
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct DisplayTransformPlan {
    pub(super) rules: Vec<TextTransformRule>,
    pub(super) macros: BTreeMap<String, String>,
}

pub(super) async fn load_display_rules<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
) -> StorageResult<HashMap<String, DisplayTransformPlan>> {
    let rows = connection.query_all_raw(sql(
        "SELECT t.id AS turn_id, ni.execution_snapshot_object_id FROM conversation_turns t JOIN turn_candidates tc ON tc.turn_id = t.id LEFT JOIN conversation_selections s ON s.turn_id = t.id JOIN node_instances ni ON ni.run_id = tc.run_id WHERE t.conversation_id = ? AND ni.execution_snapshot_object_id IS NOT NULL AND tc.run_id = COALESCE(s.selected_run_id, (SELECT first.run_id FROM turn_candidates first WHERE first.turn_id = t.id ORDER BY first.created_at, first.run_id LIMIT 1)) ORDER BY t.created_at, t.id, ni.node_id",
        vec![conversation_id.into()],
    )).await?;
    let mut candidates: HashMap<String, Option<DisplayTransformPlan>> = HashMap::new();
    for row in rows {
        let turn_id: String = row.try_get("", "turn_id")?;
        let object_id: String = row.try_get("", "execution_snapshot_object_id")?;
        let snapshot: LlmNodeExecutionSnapshot = load_object_json(connection, &object_id).await?;
        let plan = match snapshot.context {
            ContextConfigSnapshot::Preset { spec, .. }
            | ContextConfigSnapshot::GraphInline { spec, .. } => DisplayTransformPlan {
                rules: spec.text_transforms,
                macros: spec.text_transform_macros,
            },
        };
        if plan.rules.is_empty() {
            continue;
        }
        candidates
            .entry(turn_id)
            .and_modify(|current| {
                if current.as_ref().is_some_and(|existing| existing != &plan) {
                    *current = None;
                }
            })
            .or_insert(Some(plan));
    }
    Ok(candidates
        .into_iter()
        .filter_map(|(turn, plan)| plan.map(|plan| (turn, plan)))
        .collect())
}

pub(super) fn project_display_content(
    content: &[LlmContentPartIr],
    role: ConversationMessageRole,
    depth: u32,
    plan: &DisplayTransformPlan,
) -> StorageResult<Option<Vec<LlmContentPartIr>>> {
    let context = TextTransformContext {
        target: Some(match role {
            ConversationMessageRole::User => TextTransformTarget::UserInput,
            ConversationMessageRole::Assistant => TextTransformTarget::AssistantOutput,
        }),
        surface: Some(TextTransformSurface::Display),
        depth: Some(depth),
        is_edit: false,
        macros: plan.macros.clone(),
    };
    let mut projected = content.to_vec();
    for part in &mut projected {
        let LlmContentPartIr::Text { text } = part else {
            continue;
        };
        *text = apply_text_transforms(text, &plan.rules, &context)
            .map_err(|error| StorageError::InvalidArgument(error.to_string()))?
            .text;
    }
    Ok((projected != content).then_some(projected))
}
