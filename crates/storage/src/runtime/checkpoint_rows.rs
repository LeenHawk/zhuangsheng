use sea_orm::{ConnectionTrait, QueryResult};
use zhuangsheng_core::runtime_checkpoint::RuntimeCheckpointView;

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) struct RunSlice {
    pub graph_revision_id: String,
    pub context_id: String,
    pub branch_id: String,
    pub head_commit_id: String,
    pub status: String,
    pub control_epoch: u64,
    pub through_seq: u64,
    pub effect_watermark: Option<String>,
}

pub(super) async fn load_run_slice<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
) -> StorageResult<RunSlice> {
    let row = connection.query_one_raw(sql(
        "SELECT r.graph_revision_id,r.context_id,r.branch_id,r.status,r.control_epoch,b.head_commit_id,(SELECT MAX(seq) FROM run_events WHERE run_id=r.id) AS through_seq,(SELECT ea.id FROM effect_attempts ea JOIN effects e ON e.id=ea.effect_id JOIN node_instances ni ON ni.id=e.node_instance_id WHERE ni.run_id=r.id ORDER BY e.created_at DESC,ea.attempt_no DESC,ea.id DESC LIMIT 1) AS effect_watermark FROM graph_runs r JOIN context_branches b ON b.context_id=r.context_id AND b.id=r.branch_id WHERE r.id=?",
        vec![run_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "run", id: run_id.into() })?;
    Ok(RunSlice {
        graph_revision_id: row.try_get("", "graph_revision_id")?,
        context_id: row.try_get("", "context_id")?,
        branch_id: row.try_get("", "branch_id")?,
        head_commit_id: row.try_get("", "head_commit_id")?,
        status: row.try_get("", "status")?,
        control_epoch: unsigned(row.try_get("", "control_epoch")?, "run control epoch")?,
        through_seq: unsigned(row.try_get("", "through_seq")?, "runtime event sequence")?,
        effect_watermark: row.try_get("", "effect_watermark")?,
    })
}

pub(super) async fn load_at_seq<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    through_seq: u64,
) -> StorageResult<Option<RuntimeCheckpointView>> {
    connection.query_one_raw(sql(
        "SELECT id,run_id,context_branch_id,through_seq,graph_revision_id,head_commit_id,snapshot_object_id,effect_watermark,schema_version,checksum,created_at FROM runtime_checkpoints WHERE run_id=? AND through_seq=?",
        vec![run_id.into(), (through_seq as i64).into()],
    )).await?.map(|row| checkpoint_from_row(&row)).transpose()
}

pub(super) async fn load_latest<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
) -> StorageResult<Option<RuntimeCheckpointView>> {
    connection.query_one_raw(sql(
        "SELECT id,run_id,context_branch_id,through_seq,graph_revision_id,head_commit_id,snapshot_object_id,effect_watermark,schema_version,checksum,created_at FROM runtime_checkpoints WHERE run_id=? ORDER BY through_seq DESC LIMIT 1",
        vec![run_id.into()],
    )).await?.map(|row| checkpoint_from_row(&row)).transpose()
}

pub(super) async fn insert_checkpoint<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &RuntimeCheckpointView,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO runtime_checkpoints (id,run_id,context_branch_id,through_seq,graph_revision_id,head_commit_id,snapshot_object_id,effect_watermark,schema_version,checksum,created_at) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        vec![checkpoint.id.clone().into(),checkpoint.run_id.clone().into(),checkpoint.context_branch_id.clone().into(),(checkpoint.through_seq as i64).into(),checkpoint.graph_revision_id.clone().into(),checkpoint.head_commit_id.clone().into(),checkpoint.snapshot_ref.clone().into(),checkpoint.effect_watermark.clone().into(),i64::from(checkpoint.schema_version).into(),checkpoint.checksum.clone().into(),checkpoint.created_at.into()],
    )).await?;
    Ok(())
}

pub(super) async fn validate_projection<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    journal_through: u64,
) -> StorageResult<()> {
    let row = connection.query_one_raw(sql(
        "SELECT r.status,ec.next_seq,c.pending_queue_values,c.open_waits,(SELECT COUNT(*) FROM edge_queue_values q WHERE q.run_id=r.id AND q.consumed_at IS NULL) AS actual_pending,(SELECT COUNT(*) FROM node_waits w WHERE w.run_id=r.id AND w.status='open') AS actual_waits FROM graph_runs r JOIN run_event_counters ec ON ec.run_id=r.id JOIN run_execution_counters c ON c.run_id=r.id WHERE r.id=?",
        vec![run_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("runtime checkpoint projection missing".into()))?;
    let next_seq = unsigned(row.try_get("", "next_seq")?, "runtime event counter")?;
    let pending = row.try_get::<i64>("", "pending_queue_values")?;
    let waits = row.try_get::<i64>("", "open_waits")?;
    if next_seq != journal_through.saturating_add(1)
        || pending != row.try_get::<i64>("", "actual_pending")?
        || waits != row.try_get::<i64>("", "actual_waits")?
    {
        return Err(StorageError::Integrity(
            "runtime checkpoint projection diverged from journal or counters".into(),
        ));
    }
    let status: String = row.try_get("", "status")?;
    let terminal = match status.as_str() {
        "completed" => Some("run.completed"),
        "failed" => Some("run.failed"),
        "cancelled" => Some("run.cancelled"),
        _ => None,
    };
    if let Some(event_type) = terminal
        && connection
            .query_one_raw(sql(
                "SELECT 1 AS present FROM run_events WHERE run_id=? AND event_type=? LIMIT 1",
                vec![run_id.into(), event_type.into()],
            ))
            .await?
            .is_none()
    {
        return Err(StorageError::Integrity(
            "terminal run projection has no durable terminal event".into(),
        ));
    }
    Ok(())
}

fn checkpoint_from_row(row: &QueryResult) -> StorageResult<RuntimeCheckpointView> {
    Ok(RuntimeCheckpointView {
        id: row.try_get("", "id")?,
        run_id: row.try_get("", "run_id")?,
        context_branch_id: row.try_get("", "context_branch_id")?,
        through_seq: unsigned(row.try_get("", "through_seq")?, "checkpoint sequence")?,
        graph_revision_id: row.try_get("", "graph_revision_id")?,
        head_commit_id: row.try_get("", "head_commit_id")?,
        snapshot_ref: row.try_get("", "snapshot_object_id")?,
        effect_watermark: row.try_get("", "effect_watermark")?,
        schema_version: u32::try_from(row.try_get::<i64>("", "schema_version")?)
            .map_err(|_| StorageError::Integrity("invalid checkpoint schema version".into()))?,
        checksum: row.try_get("", "checksum")?,
        created_at: row.try_get("", "created_at")?,
    })
}

fn unsigned(value: i64, name: &str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::Integrity(format!("invalid {name}")))
}
