use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use zhuangsheng_core::graph::OutputCollection;
use zhuangsheng_core::runtime::{
    DurableRunEventView, RunOutputEntryView, RunOutputValueView, RunOutputsView,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{load_object_bytes, load_object_json, sql},
    },
};

const INLINE_JSON_CAP: i64 = 256 * 1024;

impl SqliteStore {
    pub async fn get_run_outputs(&self, run_id: &str) -> StorageResult<RunOutputsView> {
        let row = self
            .db
            .query_one_raw(sql(
                "SELECT graph_revision_id FROM graph_runs WHERE id = ?",
                vec![run_id.into()],
            ))
            .await?
            .ok_or_else(|| StorageError::NotFound {
                kind: "graph_run",
                id: run_id.into(),
            })?;
        let revision_id: String = row.try_get("", "graph_revision_id")?;
        let revision = load_revision(&self.db, &revision_id).await?;
        let mut result = BTreeMap::new();
        for contract in &revision.definition.output_contract {
            let rows = self.db.query_all_raw(sql(
                "SELECT o.value_object_id, c.content_hash, c.byte_size FROM run_output_values o JOIN content_objects c ON c.id = o.value_object_id WHERE o.run_id = ? AND o.output_key = ? ORDER BY o.output_seq",
                vec![run_id.into(), contract.key.clone().into()],
            )).await?;
            let mut values = Vec::new();
            for row in rows {
                let value_ref: String = row.try_get("", "value_object_id")?;
                let content_hash: String = row.try_get("", "content_hash")?;
                let size: i64 = row.try_get("", "byte_size")?;
                let size_bytes = u64::try_from(size)
                    .map_err(|_| StorageError::Integrity("negative object size".into()))?;
                if size <= INLINE_JSON_CAP {
                    values.push(RunOutputValueView::InlineJson {
                        value: load_object_json(&self.db, &value_ref).await?,
                        value_ref,
                        content_hash,
                        size_bytes,
                    });
                } else {
                    values.push(RunOutputValueView::JsonValueRef {
                        download_path: format!("/v1/values/{value_ref}"),
                        value_ref,
                        content_hash,
                        size_bytes,
                    });
                }
            }
            result.insert(
                contract.key.clone(),
                RunOutputEntryView {
                    collection: match contract.collection {
                        OutputCollection::Single => "single",
                        OutputCollection::Append => "append",
                    }
                    .into(),
                    values,
                },
            );
        }
        Ok(result)
    }

    pub async fn list_run_events(
        &self,
        run_id: &str,
        after: u64,
        limit: u32,
    ) -> StorageResult<Vec<DurableRunEventView>> {
        self.get_run(run_id).await?;
        let limit = i64::from(limit.clamp(1, 500));
        let rows = self.db.query_all_raw(sql(
            "SELECT id, run_id, seq, event_type, schema_version, created_at, node_instance_id, attempt_id, importance, payload_json, payload_object_id FROM run_events WHERE run_id = ? AND seq > ? ORDER BY seq LIMIT ?",
            vec![run_id.into(), i64::try_from(after).unwrap_or(i64::MAX).into(), limit.into()],
        )).await?;
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let payload_json: Option<String> = row.try_get("", "payload_json")?;
            let payload = if let Some(payload) = payload_json {
                serde_json::from_str(&payload)
                    .map_err(|_| StorageError::Integrity("event payload is invalid".into()))?
            } else {
                let object_id: String = row.try_get("", "payload_object_id")?;
                load_object_json(&self.db, &object_id).await?
            };
            events.push(DurableRunEventView {
                id: row.try_get("", "id")?,
                run_id: row.try_get("", "run_id")?,
                durable_seq: u64::try_from(row.try_get::<i64>("", "seq")?)
                    .map_err(|_| StorageError::Integrity("invalid event sequence".into()))?,
                event_type: row.try_get("", "event_type")?,
                schema_version: u32::try_from(row.try_get::<i64>("", "schema_version")?)
                    .map_err(|_| StorageError::Integrity("invalid event schema version".into()))?,
                timestamp: row.try_get("", "created_at")?,
                node_instance_id: row.try_get("", "node_instance_id")?,
                attempt_id: row.try_get("", "attempt_id")?,
                importance: row.try_get("", "importance")?,
                payload,
            });
        }
        Ok(events)
    }

    pub async fn load_json_value_bytes(&self, value_ref: &str) -> StorageResult<Vec<u8>> {
        let readable = self
            .db
            .query_one_raw(sql(
                "SELECT 1 AS present FROM run_output_values WHERE value_object_id = ? LIMIT 1",
                vec![value_ref.into()],
            ))
            .await?
            .is_some();
        if !readable {
            return Err(StorageError::NotFound {
                kind: "run_output_value",
                id: value_ref.into(),
            });
        }
        load_object_bytes(&self.db, value_ref).await
    }
}
