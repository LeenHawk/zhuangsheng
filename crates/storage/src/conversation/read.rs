use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::{ConversationContextV1, ConversationView};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(crate) async fn load_conversation<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
) -> StorageResult<ConversationView> {
    let row = connection.query_one_raw(sql(
        "SELECT c.id, c.title, c.context_id, c.active_branch_id, c.active_head_commit_id, c.created_at, c.updated_at, ctx.kind AS context_kind, ctx.status AS context_status, b.status AS branch_status, b.head_commit_id AS branch_head, p.head_commit_id AS projection_head, p.projection_json, vc.aggregate_id AS commit_context, vc.lineage_key AS commit_branch FROM conversations c JOIN contexts ctx ON ctx.id = c.context_id JOIN context_branches b ON b.context_id = c.context_id AND b.id = c.active_branch_id JOIN materialized_projections p ON p.aggregate_kind = 'working_context' AND p.aggregate_id = c.context_id AND p.lineage_key = c.active_branch_id JOIN version_commits vc ON vc.id = c.active_head_commit_id AND vc.aggregate_kind = 'working_context' WHERE c.id = ?",
        vec![conversation_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "conversation", id: conversation_id.into() })?;
    let active_head: String = row.try_get("", "active_head_commit_id")?;
    let context_id: String = row.try_get("", "context_id")?;
    let branch_id: String = row.try_get("", "active_branch_id")?;
    if row.try_get::<String>("", "context_kind")? != "conversation"
        || row.try_get::<String>("", "context_status")? != "active"
        || row.try_get::<String>("", "branch_status")? != "active"
        || row.try_get::<String>("", "branch_head")? != active_head
        || row.try_get::<String>("", "projection_head")? != active_head
        || row.try_get::<String>("", "commit_context")? != context_id
        || row.try_get::<String>("", "commit_branch")? != branch_id
    {
        return Err(StorageError::Integrity(
            "conversation active projection is inconsistent".into(),
        ));
    }
    let projection: ConversationContextV1 =
        serde_json::from_str(&row.try_get::<String>("", "projection_json")?)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
    projection
        .validate()
        .map_err(|message| StorageError::Integrity(message.into()))?;
    Ok(ConversationView {
        id: row.try_get("", "id")?,
        title: row.try_get("", "title")?,
        context_id,
        active_branch_id: branch_id,
        active_head_commit_id: active_head,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}
