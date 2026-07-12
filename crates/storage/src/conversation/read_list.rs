use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::ConversationListView;

use crate::{StorageResult, graph::helpers::sql};

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
    Ok(ConversationListView { items })
}
