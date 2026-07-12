use sea_orm::{ConnectionTrait, TransactionTrait};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, sql},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtifactStagingMaintenanceReport {
    pub quarantined: u64,
    pub deleting: u64,
    pub deleted: u64,
}

impl SqliteStore {
    pub async fn maintain_artifact_staging(
        &self,
        now: i64,
        quarantine_grace_ms: i64,
        limit: u32,
    ) -> StorageResult<ArtifactStagingMaintenanceReport> {
        if !(60_000..=30 * 24 * 60 * 60 * 1000).contains(&quarantine_grace_ms)
            || limit == 0
            || limit > 1_000
        {
            return Err(StorageError::InvalidArgument(
                "invalid artifact staging maintenance limits".into(),
            ));
        }
        let mut report = ArtifactStagingMaintenanceReport::default();
        let transaction = self.db.begin().await?;
        let expired = transaction.query_all_raw(sql(
            "SELECT id, status, lifecycle_generation FROM artifact_staging WHERE status IN ('uploading','staged','validated') AND (expires_at <= ? OR (status = 'uploading' AND lease_until <= ?)) ORDER BY expires_at, id LIMIT ?",
            vec![now.into(), now.into(), i64::from(limit).into()],
        )).await?;
        for row in expired {
            let id: String = row.try_get("", "id")?;
            let status: String = row.try_get("", "status")?;
            let generation: i64 = row.try_get("", "lifecycle_generation")?;
            let updated = transaction.execute_raw(sql(
                "UPDATE artifact_staging SET status = 'quarantined', lifecycle_generation = lifecycle_generation + 1, lease_until = NULL, quarantined_at = ?, updated_at = ? WHERE id = ? AND status = ? AND lifecycle_generation = ?",
                vec![now.into(), now.into(), id.into(), status.into(), generation.into()],
            )).await?;
            report.quarantined = report.quarantined.saturating_add(updated.rows_affected());
        }
        let cutoff = now.saturating_sub(quarantine_grace_ms);
        let quarantined = transaction.query_all_raw(sql(
            "SELECT id, lifecycle_generation, validated_content_object_id FROM artifact_staging WHERE status = 'quarantined' AND quarantined_at <= ? ORDER BY quarantined_at, id LIMIT ?",
            vec![cutoff.into(), i64::from(limit).into()],
        )).await?;
        for row in quarantined {
            let id: String = row.try_get("", "id")?;
            let generation: i64 = row.try_get("", "lifecycle_generation")?;
            let object_id: Option<String> = row.try_get("", "validated_content_object_id")?;
            let fence = new_id("deletefence");
            let updated = transaction.execute_raw(sql(
                "UPDATE artifact_staging SET status = 'deleting', lifecycle_generation = lifecycle_generation + 1, delete_fence = ?, validated_content_object_id = NULL, updated_at = ? WHERE id = ? AND status = 'quarantined' AND lifecycle_generation = ?",
                vec![fence.into(), now.into(), id.clone().into(), generation.into()],
            )).await?;
            if updated.rows_affected() == 1 {
                if let Some(object_id) = object_id {
                    transaction.execute_raw(sql(
                        "DELETE FROM content_object_refs WHERE object_id = ? AND owner_kind = 'artifact_staging' AND owner_id = ? AND role = 'validated_content'",
                        vec![object_id.into(), id.into()],
                    )).await?;
                }
                report.deleting = report.deleting.saturating_add(1);
            }
        }
        transaction.commit().await?;

        let transaction = self.db.begin().await?;
        let deleting = transaction.query_all_raw(sql(
            "SELECT id, lifecycle_generation, delete_fence FROM artifact_staging WHERE status = 'deleting' AND temp_storage_key IS NULL ORDER BY updated_at, id LIMIT ?",
            vec![i64::from(limit).into()],
        )).await?;
        for row in deleting {
            let id: String = row.try_get("", "id")?;
            let generation: i64 = row.try_get("", "lifecycle_generation")?;
            let fence: String = row.try_get("", "delete_fence")?;
            let updated = transaction.execute_raw(sql(
                "UPDATE artifact_staging SET status = 'deleted', lifecycle_generation = lifecycle_generation + 1, deleted_at = ?, updated_at = ? WHERE id = ? AND status = 'deleting' AND lifecycle_generation = ? AND delete_fence = ?",
                vec![now.into(), now.into(), id.into(), generation.into(), fence.into()],
            )).await?;
            report.deleted = report.deleted.saturating_add(updated.rows_affected());
        }
        transaction.commit().await?;
        Ok(report)
    }
}
