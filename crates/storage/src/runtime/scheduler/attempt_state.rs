use sea_orm::ConnectionTrait;
use zhuangsheng_core::scheduler::FinalizeAttemptCommand;

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) struct AttemptState {
    pub run_id: String,
    pub node_instance_id: String,
    pub node_id: String,
    pub graph_revision_id: String,
    pub inputs_object_id: String,
    pub status: String,
    pub worker_id: Option<String>,
    pub lease_fence: i64,
    pub run_control_epoch: i64,
    pub result_key: Option<String>,
    pub run_status: String,
    pub current_control_epoch: i64,
    pub drain_epoch: Option<i64>,
    pub lease_until: Option<i64>,
    pub attempt_deadline: Option<i64>,
    pub run_deadline: i64,
}

pub(super) async fn load_attempt<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
) -> StorageResult<AttemptState> {
    let row = connection.query_one_raw(sql(
        "SELECT a.status, a.worker_id, a.lease_fence, a.run_control_epoch, a.result_idempotency_key, a.lease_until, a.deadline_at AS attempt_deadline, ni.id AS node_instance_id, ni.run_id, ni.node_id, ni.graph_revision_id, ni.inputs_object_id, r.status AS run_status, r.control_epoch AS current_control_epoch, r.drain_epoch, r.deadline_at AS run_deadline FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE a.id = ?",
        vec![attempt_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "node_attempt", id: attempt_id.into() })?;
    Ok(AttemptState {
        run_id: row.try_get("", "run_id")?,
        node_instance_id: row.try_get("", "node_instance_id")?,
        node_id: row.try_get("", "node_id")?,
        graph_revision_id: row.try_get("", "graph_revision_id")?,
        inputs_object_id: row.try_get("", "inputs_object_id")?,
        status: row.try_get("", "status")?,
        worker_id: row.try_get("", "worker_id")?,
        lease_fence: row.try_get("", "lease_fence")?,
        run_control_epoch: row.try_get("", "run_control_epoch")?,
        result_key: row.try_get("", "result_idempotency_key")?,
        run_status: row.try_get("", "run_status")?,
        current_control_epoch: row.try_get("", "current_control_epoch")?,
        drain_epoch: row.try_get("", "drain_epoch")?,
        lease_until: row.try_get("", "lease_until")?,
        attempt_deadline: row.try_get("", "attempt_deadline")?,
        run_deadline: row.try_get("", "run_deadline")?,
    })
}

pub(super) fn validate_fence(
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    now: i64,
) -> StorageResult<()> {
    let lifecycle_accepts = (state.run_status == "running"
        && state.run_control_epoch == state.current_control_epoch)
        || (state.run_status == "interrupting"
            && state.drain_epoch == Some(state.run_control_epoch));
    if state.status != "running"
        || state.worker_id.as_deref() != Some(&command.worker_id)
        || state.lease_fence != command.lease_fence as i64
        || state.run_control_epoch != command.run_control_epoch as i64
        || !lifecycle_accepts
        || state.lease_until.is_none_or(|deadline| now >= deadline)
        || state
            .attempt_deadline
            .is_none_or(|deadline| now >= deadline)
        || now >= state.run_deadline
    {
        return Err(StorageError::Conflict("attempt_fence"));
    }
    Ok(())
}
