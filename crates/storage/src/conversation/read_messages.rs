use std::collections::HashMap;

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    canonical,
    conversation::{
        ConversationContextMessageV1, ConversationMessageRole, ConversationMessageSource,
        ConversationMessageView,
    },
    llm::ir::{LlmContentPartIr, validate_content_parts},
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) async fn load_active_messages<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    context_id: &str,
    active: &[ConversationContextMessageV1],
) -> StorageResult<Vec<ConversationMessageView>> {
    let rows = connection.query_all_raw(sql(
        "SELECT m.id, m.turn_id, m.branch_id, m.commit_id, m.parent_message_id, m.role, m.source_kind, m.content_object_id, m.origin_run_id, m.created_at, o.lifecycle, o.content_hash, o.byte_size, o.inline_bytes, vc.aggregate_id AS commit_context, vc.lineage_key AS commit_branch, EXISTS (SELECT 1 FROM content_object_refs r WHERE r.object_id = m.content_object_id AND r.owner_kind = 'conversation_message' AND r.owner_id = m.id AND r.role = 'content') AS content_ref FROM conversation_messages m JOIN content_objects o ON o.id = m.content_object_id JOIN version_commits vc ON vc.id = m.commit_id AND vc.aggregate_kind = 'working_context' WHERE m.conversation_id = ?",
        vec![conversation_id.into()],
    )).await?;
    let mut by_id = HashMap::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("", "id")?;
        if by_id.insert(id.clone(), row).is_some() {
            return Err(StorageError::Integrity(
                "conversation message identity is duplicated".into(),
            ));
        }
    }
    let mut messages = Vec::with_capacity(active.len());
    for expected in active {
        let row = by_id.get(&expected.message_id).ok_or_else(|| {
            StorageError::Integrity("active conversation message row is missing".into())
        })?;
        let role = parse_role(&row.try_get::<String>("", "role")?)?;
        let source = parse_source(&row.try_get::<String>("", "source_kind")?)?;
        let bytes: Vec<u8> = row.try_get("", "inline_bytes")?;
        let content: Vec<LlmContentPartIr> = serde_json::from_slice(&bytes)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
        validate_content_parts(&content, true)
            .map_err(|_| StorageError::Integrity("conversation content is invalid".into()))?;
        let parent: Option<String> = row.try_get("", "parent_message_id")?;
        let origin: Option<String> = row.try_get("", "origin_run_id")?;
        let branch_id: String = row.try_get("", "branch_id")?;
        if expected.turn_id != row.try_get::<String>("", "turn_id")?
            || expected.content_ref != row.try_get::<String>("", "content_object_id")?
            || expected.parent_message_id != parent
            || expected.origin_run_id != origin
            || expected.role != role
            || expected.source != source
            || row.try_get::<String>("", "lifecycle")? != "live"
            || row.try_get::<String>("", "content_hash")? != canonical::hash_bytes(&bytes)
            || row.try_get::<i64>("", "byte_size")? != bytes.len() as i64
            || row.try_get::<String>("", "commit_context")? != context_id
            || row.try_get::<String>("", "commit_branch")? != branch_id
            || row.try_get::<i64>("", "content_ref")? != 1
        {
            return Err(StorageError::Integrity(
                "active conversation message is inconsistent".into(),
            ));
        }
        messages.push(ConversationMessageView {
            id: expected.message_id.clone(),
            turn_id: expected.turn_id.clone(),
            branch_id,
            commit_id: row.try_get("", "commit_id")?,
            parent_message_id: parent,
            role,
            source,
            content,
            origin_run_id: origin,
            created_at: row.try_get("", "created_at")?,
        });
    }
    Ok(messages)
}

fn parse_role(value: &str) -> StorageResult<ConversationMessageRole> {
    match value {
        "user" => Ok(ConversationMessageRole::User),
        "assistant" => Ok(ConversationMessageRole::Assistant),
        _ => Err(StorageError::Integrity("invalid conversation role".into())),
    }
}

fn parse_source(value: &str) -> StorageResult<ConversationMessageSource> {
    match value {
        "user_input" => Ok(ConversationMessageSource::UserInput),
        "run_output" => Ok(ConversationMessageSource::RunOutput),
        "saved_partial" => Ok(ConversationMessageSource::SavedPartial),
        _ => Err(StorageError::Integrity(
            "invalid conversation message source".into(),
        )),
    }
}
