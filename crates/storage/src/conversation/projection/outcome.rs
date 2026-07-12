use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::canonical;

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

pub(super) async fn projection_conflict<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    message: &str,
    now: i64,
) -> StorageResult<()> {
    finish_with_error(
        connection,
        run_id,
        "projection_conflicted",
        "conflicted",
        message,
        now,
    )
    .await
}

pub(super) async fn permanent_failure<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    message: &str,
    now: i64,
) -> StorageResult<()> {
    finish_with_error(
        connection,
        run_id,
        "projection_failed",
        "failed",
        message,
        now,
    )
    .await
}

async fn finish_with_error<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    candidate_status: &str,
    job_status: &str,
    message: &str,
    now: i64,
) -> StorageResult<()> {
    let error_id = put_inline_object(
        connection,
        &canonical::to_vec(
            &json!({"schemaVersion":1,"code":candidate_status,"safeMessage":message}),
        )?,
        now,
    )
    .await?;
    let updated = connection.execute_raw(sql(
        "UPDATE turn_candidates SET status = ?, projection_error_object_id = ? WHERE run_id = ? AND status = 'running'",
        vec![candidate_status.into(), error_id.clone().into(), run_id.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("candidate_status"));
    }
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'turn_candidate', ?, 'projection_error', ?)",
        vec![error_id.clone().into(), run_id.into(), now.into()],
    )).await?;
    finish_job(connection, run_id, job_status, Some(&error_id), now).await
}

pub(super) async fn finish_job<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    status: &str,
    error_id: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    let updated = connection.execute_raw(sql(
        "UPDATE candidate_projection_jobs SET status = ?, claimed_by = NULL, lease_until = NULL, last_error_object_id = ?, completed_at = ? WHERE run_id = ? AND status = 'claimed'",
        vec![status.into(), error_id.map(String::from).into(), now.into(), run_id.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("candidate_projection_claim"));
    }
    Ok(())
}
