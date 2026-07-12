use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::context::WorkingContextView,
    canonical,
    conversation::{ConversationContextV1, ConversationMessageRole},
    llm::{
        context::{ContextProvenance, ResolvedContextValue},
        ir::{ContextSensitivity, ContextTrust, MessageRole},
    },
};

use crate::{StorageError, StorageResult, conversation::load_active_messages, graph::helpers::sql};

use super::read_set::{ResolvedBinding, ResolvedSelection};

pub(super) async fn resolve<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    branch_id: &str,
    context: &WorkingContextView,
) -> StorageResult<ResolvedBinding> {
    let projection: ConversationContextV1 = serde_json::from_value(context.value.clone())
        .map_err(|error| StorageError::InputContract(error.to_string()))?;
    projection
        .validate()
        .map_err(|message| StorageError::InputContract(message.into()))?;
    let conversation_id = connection
        .query_one_raw(sql(
            "SELECT id FROM conversations WHERE context_id = ?",
            vec![context_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            StorageError::InputContract(
                "conversation history requires a conversation context".into(),
            )
        })?
        .try_get::<String>("", "id")?;
    let messages = load_active_messages(
        connection,
        &conversation_id,
        context_id,
        &projection.messages,
    )
    .await?;
    let values = messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| {
            let role = match message.role {
                ConversationMessageRole::User => MessageRole::User,
                ConversationMessageRole::Assistant => MessageRole::Assistant,
            };
            Ok(ResolvedContextValue::HistoryMessage {
                message_id: message.id.clone(),
                turn_id: message.turn_id,
                stable_order: u64::try_from(index).map_err(|_| {
                    StorageError::Integrity("conversation history order overflow".into())
                })?,
                role,
                content_hash: canonical::hash(&message.content)?,
                content: message.content,
                provenance: ContextProvenance {
                    source_type: "conversation_message".into(),
                    source_id: message.id,
                    trust: ContextTrust::ExternalUntrusted,
                    sensitivity: ContextSensitivity::Private,
                },
            })
        })
        .collect::<StorageResult<Vec<_>>>()?;
    let values_hash = canonical::hash(&values)?;
    Ok(ResolvedBinding {
        envelope: json!({
            "kind":"conversation_history",
            "commitId":context.head_commit_id,
            "values":values,
        }),
        selections: vec![ResolvedSelection {
            aggregate_kind: "working_context",
            aggregate_id: context_id.into(),
            lineage_key: branch_id.into(),
            commit_id: context.head_commit_id.clone(),
            selection_ordinal: None,
            content_hash: Some(values_hash),
        }],
        scope_snapshot_token: None,
        truncated: false,
    })
}
