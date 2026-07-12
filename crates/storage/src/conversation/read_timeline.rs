use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::{ConversationContextV1, ConversationTimelineView};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::{
    read::load_conversation, read_candidates::load_active_turns,
    read_messages::load_active_messages,
};

pub(super) async fn load_timeline<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
) -> StorageResult<ConversationTimelineView> {
    let conversation = load_conversation(connection, conversation_id).await?;
    let row = connection.query_one_raw(sql(
        "SELECT projection_json FROM materialized_projections WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND head_commit_id = ?",
        vec![conversation.context_id.clone().into(), conversation.active_branch_id.clone().into(), conversation.active_head_commit_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::Integrity("active conversation projection is missing".into()))?;
    let context: ConversationContextV1 =
        serde_json::from_str(&row.try_get::<String>("", "projection_json")?)
            .map_err(|error| StorageError::Integrity(error.to_string()))?;
    context
        .validate()
        .map_err(|message| StorageError::Integrity(message.into()))?;
    let messages = load_active_messages(
        connection,
        conversation_id,
        &conversation.context_id,
        &context.messages,
    )
    .await?;
    let turns = load_active_turns(connection, conversation_id, &messages).await?;
    Ok(ConversationTimelineView {
        conversation_id: conversation.id,
        active_branch_id: conversation.active_branch_id,
        active_head_commit_id: conversation.active_head_commit_id,
        messages,
        turns,
    })
}
