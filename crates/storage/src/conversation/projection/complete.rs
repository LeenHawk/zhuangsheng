use sea_orm::ConnectionTrait;

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::{append::project_completed, outcome::finish_job};

pub(super) struct Candidate {
    pub terminal_status: String,
    pub turn_id: String,
    pub branch_id: String,
    pub reply_output_key: String,
    pub user_message_id: String,
    pub conversation_id: String,
    pub context_id: String,
    pub output_commit_id: Option<String>,
}

pub(super) async fn project_claimed<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    worker_id: &str,
    now: i64,
) -> StorageResult<()> {
    let candidate = load_candidate(connection, run_id, worker_id).await?;
    match candidate.terminal_status.as_str() {
        "failed" | "cancelled" => finish_terminal(connection, run_id, &candidate, now).await,
        "completed" => project_completed(connection, run_id, candidate, now).await,
        _ => Err(StorageError::Integrity(
            "projection job terminal status is invalid".into(),
        )),
    }
}

async fn load_candidate<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    worker_id: &str,
) -> StorageResult<Candidate> {
    let row = connection.query_one_raw(sql(
        "SELECT j.terminal_status, tc.turn_id, tc.branch_id, tc.reply_output_key, t.user_message_id, t.conversation_id, c.context_id, r.output_commit_id, r.status AS run_status, tc.status AS candidate_status FROM candidate_projection_jobs j JOIN turn_candidates tc ON tc.run_id = j.run_id JOIN conversation_turns t ON t.id = tc.turn_id JOIN conversations c ON c.id = t.conversation_id JOIN graph_runs r ON r.id = j.run_id WHERE j.run_id = ? AND j.status = 'claimed' AND j.claimed_by = ?",
        vec![run_id.into(), worker_id.into()],
    )).await?.ok_or_else(|| StorageError::Conflict("candidate_projection_claim"))?;
    let terminal_status: String = row.try_get("", "terminal_status")?;
    if row.try_get::<String>("", "run_status")? != terminal_status
        || row.try_get::<String>("", "candidate_status")? != "running"
    {
        return Err(StorageError::Integrity(
            "candidate projection source changed".into(),
        ));
    }
    Ok(Candidate {
        terminal_status,
        turn_id: row.try_get("", "turn_id")?,
        branch_id: row.try_get("", "branch_id")?,
        reply_output_key: row.try_get("", "reply_output_key")?,
        user_message_id: row.try_get("", "user_message_id")?,
        conversation_id: row.try_get("", "conversation_id")?,
        context_id: row.try_get("", "context_id")?,
        output_commit_id: row.try_get("", "output_commit_id")?,
    })
}

async fn finish_terminal<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    candidate: &Candidate,
    now: i64,
) -> StorageResult<()> {
    let updated = connection
        .execute_raw(sql(
            "UPDATE turn_candidates SET status = ? WHERE run_id = ? AND status = 'running'",
            vec![candidate.terminal_status.clone().into(), run_id.into()],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("candidate_status"));
    }
    finish_job(connection, run_id, "done", None, now).await
}
