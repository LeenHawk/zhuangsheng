use sea_orm::ConnectionTrait;
use zhuangsheng_core::{canonical, context_merge::MergeContextView};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
};

pub(super) async fn replay<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<MergeContextView>> {
    let row = connection.query_one_raw(sql(
        "SELECT request_digest, result_object_id FROM application_command_receipts WHERE scope = ? AND idempotency_key = ? AND status = 'completed'",
        vec![scope.into(), key.into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    load_object_json(connection, &row.try_get::<String>("", "result_object_id")?)
        .await
        .map(Some)
}

pub(super) async fn finish_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
    result: &MergeContextView,
    now: i64,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(result)?, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES (?, ?, ?, 'context.merge', 'context_merge', ?, 'completed', ?, ?, ?)",
        vec![scope.into(), key.into(), digest.into(), result.merge_commit_id.clone().unwrap_or_else(|| result.base_commit_id.clone()).into(), object_id.clone().into(), now.into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'application_receipt', ?, 'result', ?)",
        vec![object_id.into(), format!("{scope}:{key}").into(), now.into()],
    )).await?;
    Ok(())
}
