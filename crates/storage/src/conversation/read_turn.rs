use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::ConversationTurnDetailView;

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::read_timeline::load_timeline;

pub(super) async fn load_turn_candidates<C: ConnectionTrait>(
    connection: &C,
    turn_id: &str,
) -> StorageResult<ConversationTurnDetailView> {
    let row = connection
        .query_one_raw(sql(
            "SELECT conversation_id FROM conversation_turns WHERE id = ?",
            vec![turn_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "conversation_turn",
            id: turn_id.into(),
        })?;
    let conversation_id: String = row.try_get("", "conversation_id")?;
    load_timeline(connection, &conversation_id)
        .await?
        .turns
        .into_iter()
        .find(|turn| turn.turn.id == turn_id)
        .ok_or_else(|| {
            StorageError::Integrity("conversation turn is not in the active timeline".into())
        })
}
