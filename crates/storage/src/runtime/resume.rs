use sea_orm::ConnectionTrait;

use crate::{
    StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{new_id, sql},
    },
};

use super::scheduler::copy_attempt_reads;

pub(crate) struct ResumeAttempt<'a> {
    pub node_instance_id: &'a str,
    pub source_attempt_id: &'a str,
    pub run_id: &'a str,
    pub control_epoch: u64,
    pub idempotency_key: &'a str,
}

pub(crate) async fn create_resume_attempt<C: ConnectionTrait>(
    connection: &C,
    resume: ResumeAttempt<'_>,
    now: i64,
) -> StorageResult<String> {
    let row = connection.query_one_raw(sql(
        "SELECT a.executor_object_id, a.retry_ordinal, ni.graph_revision_id, COALESCE(MAX(all_attempts.attempt_no), 0) AS max_attempt_no FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id LEFT JOIN node_attempts all_attempts ON all_attempts.node_instance_id = ni.id WHERE a.id = ? AND a.node_instance_id = ? GROUP BY a.id, ni.id",
        vec![resume.source_attempt_id.into(), resume.node_instance_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("wait attempt is unavailable".into()))?;
    let revision =
        load_revision(connection, &row.try_get::<String>("", "graph_revision_id")?).await?;
    let next_attempt_no = row
        .try_get::<i64>("", "max_attempt_no")?
        .checked_add(1)
        .ok_or_else(|| StorageError::Integrity("node attempt number overflow".into()))?;
    if u64::try_from(next_attempt_no).ok()
        > Some(revision.definition.limits.max_attempts_per_activation)
    {
        return Err(StorageError::InvalidArgument(
            "node attempt limit prevents wait resume".into(),
        ));
    }
    let attempt_id = new_id("attempt");
    connection.execute_raw(sql(
        "INSERT INTO node_attempts (id, node_instance_id, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, idempotency_key, executor_object_id) VALUES (?, ?, ?, ?, 'resume', 'queued', ?, 0, ?, ?)",
        vec![
            attempt_id.clone().into(),
            resume.node_instance_id.into(),
            next_attempt_no.into(),
            row.try_get::<i64>("", "retry_ordinal")?.into(),
            i64::try_from(resume.control_epoch)
                .map_err(|_| StorageError::Integrity("run control epoch overflow".into()))?
                .into(),
            resume.idempotency_key.into(),
            row.try_get::<String>("", "executor_object_id")?.into(),
        ],
    )).await?;
    copy_attempt_reads(connection, resume.source_attempt_id, &attempt_id, now).await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET total_attempts = total_attempts + 1 WHERE run_id = ?",
            vec![resume.run_id.into()],
        ))
        .await?;
    Ok(attempt_id)
}
