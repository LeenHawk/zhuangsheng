use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::{
    ConversationAttentionKind, ConversationAttentionView, ConversationListView,
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::read::load_conversation;

pub(super) async fn load_conversations<C: ConnectionTrait>(
    connection: &C,
) -> StorageResult<ConversationListView> {
    let rows = connection
        .query_all_raw(sql(
            "SELECT id FROM conversations ORDER BY updated_at DESC, id",
            vec![],
        ))
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(load_conversation(connection, &row.try_get::<String>("", "id")?).await?);
    }
    let rows = connection
        .query_all_raw(sql(
            r#"SELECT ct.conversation_id, tc.run_id, w.id AS wait_id,
                      w.kind, w.created_at
               FROM node_waits w
               JOIN turn_candidates tc ON tc.run_id = w.run_id
               JOIN conversation_turns ct ON ct.id = tc.turn_id
               WHERE w.status = 'open'
               UNION ALL
               SELECT ct.conversation_id, tc.run_id, NULL AS wait_id,
                      'projection_conflict' AS kind, tc.created_at
               FROM turn_candidates tc
               JOIN conversation_turns ct ON ct.id = tc.turn_id
               WHERE tc.status = 'projection_conflicted'
               ORDER BY 5 DESC, 1, 2"#,
            vec![],
        ))
        .await?;
    let mut attention = Vec::with_capacity(rows.len());
    for row in rows {
        let raw_kind: String = row.try_get("", "kind")?;
        let kind = match raw_kind.as_str() {
            "tool_approval" => ConversationAttentionKind::ToolApproval,
            "human_response" => ConversationAttentionKind::HumanResponse,
            "memory_proposal_review" => ConversationAttentionKind::MemoryProposalReview,
            "secret_store_unlocked" => ConversationAttentionKind::SecretStoreUnlocked,
            "effect_resolution" => ConversationAttentionKind::EffectResolution,
            "projection_conflict" => ConversationAttentionKind::ProjectionConflict,
            _ => {
                return Err(StorageError::Integrity(format!(
                    "unsupported conversation attention kind: {raw_kind}"
                )));
            }
        };
        attention.push(ConversationAttentionView {
            conversation_id: row.try_get("", "conversation_id")?,
            run_id: row.try_get("", "run_id")?,
            wait_id: row.try_get("", "wait_id")?,
            kind,
            created_at: row.try_get("", "created_at")?,
        });
    }
    Ok(ConversationListView { items, attention })
}
