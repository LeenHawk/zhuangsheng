use sea_orm::ConnectionTrait;
use zhuangsheng_core::application::artifact::ArtifactListView;

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::read::load_artifact_metadata_view;

impl SqliteStore {
    pub async fn list_artifact_views(&self, limit: u32) -> StorageResult<ArtifactListView> {
        if limit == 0 || limit > 100 {
            return Err(StorageError::InvalidArgument(
                "artifact list limit must be between 1 and 100".into(),
            ));
        }
        let rows = self
            .db
            .query_all_raw(sql(
                "SELECT id FROM artifacts WHERE status = 'active' ORDER BY created_at DESC, id DESC LIMIT ?",
                vec![i64::from(limit).into()],
            ))
            .await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("", "id")?;
            items.push(load_artifact_metadata_view(&self.db, &id).await?.0);
        }
        Ok(ArtifactListView { items })
    }
}
