use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, sql},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContentObjectMaintenanceReport {
    pub scanned: u64,
    pub deleted: u64,
    pub rooted_without_owner_ref: u64,
}

impl SqliteStore {
    pub async fn maintain_content_objects(
        &self,
        now: i64,
        orphan_grace_ms: i64,
        limit: u32,
    ) -> StorageResult<ContentObjectMaintenanceReport> {
        validate(orphan_grace_ms, limit)?;
        let transaction = self.db.begin().await?;
        let foreign_keys = content_object_foreign_keys(&transaction).await?;
        let cutoff = now.saturating_sub(orphan_grace_ms);
        let candidates = transaction.query_all_raw(sql(
            "SELECT id, lifecycle_generation FROM content_objects o WHERE o.lifecycle = 'live' AND o.storage_kind = 'inline' AND o.created_at <= ? AND NOT EXISTS (SELECT 1 FROM content_object_refs r WHERE r.object_id = o.id) ORDER BY o.created_at, o.id LIMIT ?",
            vec![cutoff.into(), i64::from(limit).into()],
        )).await?;
        let mut report = ContentObjectMaintenanceReport {
            scanned: candidates.len() as u64,
            ..ContentObjectMaintenanceReport::default()
        };
        for row in candidates {
            let object_id: String = row.try_get("", "id")?;
            let generation: i64 = row.try_get("", "lifecycle_generation")?;
            if has_foreign_key_root(&transaction, &foreign_keys, &object_id).await? {
                report.rooted_without_owner_ref += 1;
                continue;
            }
            let fence = new_id("deletefence");
            let marked = transaction.execute_raw(sql(
                "UPDATE content_objects SET lifecycle = 'deleting', lifecycle_generation = lifecycle_generation + 1, delete_fence = ? WHERE id = ? AND lifecycle = 'live' AND lifecycle_generation = ? AND NOT EXISTS (SELECT 1 FROM content_object_refs WHERE object_id = ?)",
                vec![fence.clone().into(), object_id.clone().into(), generation.into(), object_id.clone().into()],
            )).await?;
            if marked.rows_affected() != 1 {
                continue;
            }
            if has_foreign_key_root(&transaction, &foreign_keys, &object_id).await? {
                return Err(StorageError::Conflict("content_object_gc_root_changed"));
            }
            let swept = transaction.execute_raw(sql(
                "UPDATE content_objects SET lifecycle = 'deleted', lifecycle_generation = lifecycle_generation + 1, inline_bytes = NULL, deleted_at = ? WHERE id = ? AND lifecycle = 'deleting' AND lifecycle_generation = ? AND delete_fence = ? AND NOT EXISTS (SELECT 1 FROM content_object_refs WHERE object_id = ?)",
                vec![now.into(), object_id.clone().into(), (generation + 1).into(), fence.into(), object_id.into()],
            )).await?;
            if swept.rows_affected() != 1 {
                return Err(StorageError::Conflict("content_object_gc_fence"));
            }
            report.deleted += 1;
        }
        transaction.commit().await?;
        Ok(report)
    }
}

async fn content_object_foreign_keys<C: ConnectionTrait>(
    connection: &C,
) -> StorageResult<Vec<(String, String)>> {
    let rows = connection.query_all_raw(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT DISTINCT m.name AS table_name, fk.\"from\" AS column_name FROM sqlite_master m, pragma_foreign_key_list(m.name) fk WHERE m.type = 'table' AND fk.\"table\" = 'content_objects' AND m.name != 'content_object_refs' ORDER BY m.name, fk.\"from\"",
    )).await?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("", "table_name")?,
                row.try_get("", "column_name")?,
            ))
        })
        .collect()
}

async fn has_foreign_key_root<C: ConnectionTrait>(
    connection: &C,
    foreign_keys: &[(String, String)],
    object_id: &str,
) -> StorageResult<bool> {
    for (table, column) in foreign_keys {
        let query = format!(
            "SELECT 1 AS present FROM {} WHERE {} = ? LIMIT 1",
            quote_identifier(table),
            quote_identifier(column)
        );
        if connection
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                query,
                vec![object_id.into()],
            ))
            .await?
            .is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn validate(orphan_grace_ms: i64, limit: u32) -> StorageResult<()> {
    if !(60_000..=30 * 24 * 60 * 60 * 1_000).contains(&orphan_grace_ms)
        || limit == 0
        || limit > 1_000
    {
        return Err(StorageError::InvalidArgument(
            "invalid content object maintenance limits".into(),
        ));
    }
    Ok(())
}
