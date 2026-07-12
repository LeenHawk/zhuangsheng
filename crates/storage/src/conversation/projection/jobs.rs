use sea_orm::{ConnectionTrait, TransactionTrait};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::complete::project_claimed;

pub(super) async fn maintain(
    store: &SqliteStore,
    now: i64,
    worker_id: &str,
    limit: u32,
) -> StorageResult<u64> {
    if worker_id.is_empty() || worker_id.len() > 128 {
        return Err(StorageError::InvalidArgument(
            "invalid projection worker id".into(),
        ));
    }
    reconcile(store, now).await?;
    let rows = store.db.query_all_raw(sql(
        "SELECT run_id FROM candidate_projection_jobs WHERE status = 'pending' AND available_at <= ? ORDER BY available_at, run_id LIMIT ?",
        vec![now.into(), i64::from(limit).into()],
    )).await?;
    let mut processed = 0;
    for row in rows {
        let run_id: String = row.try_get("", "run_id")?;
        let transaction = store.db.begin().await?;
        let claimed = transaction.execute_raw(sql(
            "UPDATE candidate_projection_jobs SET status = 'claimed', claimed_by = ?, lease_until = ?, attempt_count = attempt_count + 1 WHERE run_id = ? AND status = 'pending' AND available_at <= ?",
            vec![worker_id.into(), now.saturating_add(30_000).into(), run_id.clone().into(), now.into()],
        )).await?;
        if claimed.rows_affected() == 0 {
            transaction.rollback().await?;
            continue;
        }
        project_claimed(&transaction, &run_id, worker_id, now).await?;
        transaction.commit().await?;
        processed += 1;
    }
    Ok(processed)
}

async fn reconcile(store: &SqliteStore, now: i64) -> StorageResult<()> {
    let transaction = store.db.begin().await?;
    transaction.execute_raw(sql(
        "UPDATE candidate_projection_jobs SET status = 'pending', claimed_by = NULL, lease_until = NULL, available_at = ? WHERE status = 'claimed' AND lease_until <= ?",
        vec![now.into(), now.into()],
    )).await?;
    transaction.execute_raw(sql(
        "INSERT OR IGNORE INTO candidate_projection_jobs (run_id, terminal_event_seq, terminal_status, status, available_at, attempt_count, created_at) SELECT tc.run_id, e.seq, r.status, 'pending', ?, 0, ? FROM turn_candidates tc JOIN graph_runs r ON r.id = tc.run_id JOIN run_events e ON e.run_id = r.id AND e.event_type = CASE r.status WHEN 'completed' THEN 'run.completed' WHEN 'failed' THEN 'run.failed' WHEN 'cancelled' THEN 'run.cancelled' END WHERE tc.status = 'running' AND r.status IN ('completed','failed','cancelled') AND e.seq = (SELECT MAX(e2.seq) FROM run_events e2 WHERE e2.run_id = r.id AND e2.event_type = e.event_type)",
        vec![now.into(), now.into()],
    )).await?;
    transaction.commit().await?;
    Ok(())
}
