use std::time::{SystemTime, UNIX_EPOCH};

use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::de::DeserializeOwned;
use ulid::Ulid;
use zhuangsheng_core::canonical;

use crate::{StorageError, StorageResult, graph::GraphView};

pub struct Receipt {
    pub digest: String,
    pub resource_id: Option<String>,
    pub result_object_id: Option<String>,
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Ulid::new())
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as i64
}

pub fn sql(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

pub async fn find_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
) -> StorageResult<Option<Receipt>> {
    let row = connection
        .query_one(sql(
            "SELECT request_digest, resource_id, result_object_id FROM application_command_receipts WHERE scope = ? AND idempotency_key = ?",
            vec![scope.into(), key.into()],
        ))
        .await?;
    row.map(|row| {
        Ok(Receipt {
            digest: row.try_get("", "request_digest")?,
            resource_id: row.try_get("", "resource_id")?,
            result_object_id: row.try_get("", "result_object_id")?,
        })
    })
    .transpose()
}

pub async fn load_object_json<C: ConnectionTrait, T: DeserializeOwned>(
    connection: &C,
    id: &str,
) -> StorageResult<T> {
    let row = connection
        .query_one(sql(
            "SELECT inline_bytes FROM content_objects WHERE id = ? AND lifecycle = 'live'",
            vec![id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity(format!("content object unavailable: {id}")))?;
    let bytes: Vec<u8> = row.try_get("", "inline_bytes")?;
    serde_json::from_slice(&bytes).map_err(|error| StorageError::Integrity(error.to_string()))
}

pub async fn load_graph<C: ConnectionTrait>(connection: &C, id: &str) -> StorageResult<GraphView> {
    let row = connection
        .query_one(sql(
            "SELECT id, name, created_at, updated_at FROM graphs WHERE id = ?",
            vec![id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "graph",
            id: id.into(),
        })?;
    graph_from_row(&row)
}

pub fn graph_from_row(row: &QueryResult) -> StorageResult<GraphView> {
    Ok(GraphView {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn put_inline_object<C: ConnectionTrait>(
    connection: &C,
    bytes: &[u8],
    created_at: i64,
) -> StorageResult<String> {
    let content_hash = canonical::hash_bytes(bytes);
    if let Some(row) = connection
        .query_one(sql(
            "SELECT id FROM content_objects WHERE content_hash = ?",
            vec![content_hash.clone().into()],
        ))
        .await?
    {
        return Ok(row.try_get("", "id")?);
    }
    let id = new_id("object");
    connection
        .execute(sql(
            "INSERT INTO content_objects (id, content_hash, byte_size, storage_kind, lifecycle, lifecycle_generation, inline_bytes, created_at) VALUES (?, ?, ?, 'inline', 'live', 0, ?, ?)",
            vec![id.clone().into(), content_hash.into(), (bytes.len() as i64).into(), bytes.to_vec().into(), created_at.into()],
        ))
        .await?;
    Ok(id)
}
