use sea_orm::{ConnectionTrait, QueryResult};
use zhuangsheng_core::runtime::{RunStatus, RunView};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

impl SqliteStore {
    pub async fn get_run(&self, run_id: &str) -> StorageResult<RunView> {
        load_run(&self.db, run_id).await
    }
}

pub(super) async fn load_run<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
) -> StorageResult<RunView> {
    let row = connection
        .query_one(sql(
            "SELECT r.id, r.graph_revision_id, r.status, r.control_epoch, r.context_id, r.branch_id, r.input_commit_id, r.run_input_object_id, r.output_commit_id, r.deadline_at, r.created_at, r.updated_at, e.next_seq - 1 AS last_durable_seq FROM graph_runs r JOIN run_event_counters e ON e.run_id = r.id WHERE r.id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "graph_run",
            id: run_id.into(),
        })?;
    run_from_row(&row)
}

pub(super) fn run_from_row(row: &QueryResult) -> StorageResult<RunView> {
    let status: String = row.try_get("", "status")?;
    Ok(RunView {
        id: row.try_get("", "id")?,
        graph_revision_id: row.try_get("", "graph_revision_id")?,
        status: parse_status(&status)?,
        control_epoch: nonnegative(row.try_get("", "control_epoch")?, "control_epoch")?,
        context_id: row.try_get("", "context_id")?,
        branch_id: row.try_get("", "branch_id")?,
        input_commit_id: row.try_get("", "input_commit_id")?,
        input_ref: row.try_get("", "run_input_object_id")?,
        output_commit_id: row.try_get("", "output_commit_id")?,
        last_durable_seq: nonnegative(row.try_get("", "last_durable_seq")?, "last_durable_seq")?,
        deadline_at: row.try_get("", "deadline_at")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn parse_status(status: &str) -> StorageResult<RunStatus> {
    match status {
        "created" => Ok(RunStatus::Created),
        "running" => Ok(RunStatus::Running),
        "waiting" => Ok(RunStatus::Waiting),
        "interrupting" => Ok(RunStatus::Interrupting),
        "interrupted" => Ok(RunStatus::Interrupted),
        "completed" => Ok(RunStatus::Completed),
        "failed" => Ok(RunStatus::Failed),
        "cancelled" => Ok(RunStatus::Cancelled),
        _ => Err(StorageError::Integrity("unknown run status".into())),
    }
}

fn nonnegative(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::Integrity(format!("negative {field}")))
}
