use sea_orm::ConnectionTrait;

use crate::{SqliteStore, StorageResult};

use super::{GraphView, helpers::*};

impl SqliteStore {
    pub async fn get_graph(&self, id: &str) -> StorageResult<GraphView> {
        load_graph(&self.db, id).await
    }

    pub async fn list_graphs(&self) -> StorageResult<Vec<GraphView>> {
        self.db
            .query_all_raw(sql(
                "SELECT id, name, created_at, updated_at FROM graphs ORDER BY created_at, id",
                vec![],
            ))
            .await?
            .iter()
            .map(graph_from_row)
            .collect()
    }
}
