use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::context::{CreateVersionSnapshotCommand, VersionSnapshotView},
    canonical,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{now_ms, put_inline_object, sql},
};

use super::replay::reconstruct;

impl SqliteStore {
    pub async fn create_version_snapshot(
        &self,
        command: CreateVersionSnapshotCommand,
    ) -> StorageResult<VersionSnapshotView> {
        let now = now_ms();
        if command.retention_until.is_some_and(|until| until <= now) {
            return Err(StorageError::InvalidArgument(
                "snapshot retention must be in the future".into(),
            ));
        }
        let transaction = self.db.begin().await?;
        if let Some(existing) = load_snapshot(&transaction, &command.commit_id).await? {
            if existing.retention_until != command.retention_until
                || existing.pinned != command.pinned
            {
                return Err(StorageError::Conflict("version_snapshot_options"));
            }
            transaction.commit().await?;
            return Ok(existing);
        }
        let reconstructed = reconstruct(&transaction, &command.commit_id).await?;
        let mut append_ids: Vec<_> = reconstructed.append_ids.into_iter().collect();
        append_ids.sort();
        let envelope = json!({
            "schemaVersion":1,
            "value":reconstructed.value,
            "appendElementIds":append_ids,
        });
        let bytes = canonical::to_vec(&envelope)?;
        let checksum = canonical::hash_bytes(&bytes);
        let object_id = put_inline_object(&transaction, &bytes, now).await?;
        transaction.execute_raw(sql(
            "INSERT INTO version_snapshots (commit_id, snapshot_object_id, schema_version, checksum, retention_until, pinned, created_at) VALUES (?, ?, 1, ?, ?, ?, ?)",
            vec![command.commit_id.clone().into(), object_id.clone().into(), checksum.clone().into(), command.retention_until.into(), i64::from(command.pinned).into(), now.into()],
        )).await?;
        transaction.execute_raw(sql(
            "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'version_snapshot', ?, 'snapshot', ?)",
            vec![object_id.into(), command.commit_id.clone().into(), now.into()],
        )).await?;
        let view = load_snapshot(&transaction, &command.commit_id)
            .await?
            .expect("snapshot was inserted");
        transaction.commit().await?;
        Ok(view)
    }
}

pub(crate) async fn load_snapshot<C: ConnectionTrait>(
    connection: &C,
    commit_id: &str,
) -> StorageResult<Option<VersionSnapshotView>> {
    let row = connection.query_one_raw(sql(
        "SELECT commit_id, snapshot_object_id, checksum, schema_version, retention_until, pinned, created_at FROM version_snapshots WHERE commit_id = ?",
        vec![commit_id.into()],
    )).await?;
    row.map(|row| {
        Ok(VersionSnapshotView {
            commit_id: row.try_get("", "commit_id")?,
            snapshot_ref: row.try_get("", "snapshot_object_id")?,
            checksum: row.try_get("", "checksum")?,
            schema_version: u32::try_from(row.try_get::<i64>("", "schema_version")?)
                .map_err(|_| StorageError::Integrity("invalid snapshot schema version".into()))?,
            retention_until: row.try_get("", "retention_until")?,
            pinned: row.try_get::<i64>("", "pinned")? == 1,
            created_at: row.try_get("", "created_at")?,
        })
    })
    .transpose()
}
