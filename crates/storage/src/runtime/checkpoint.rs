use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    runtime_checkpoint::{RuntimeCheckpointView, RuntimeRecoveryView},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

use super::{
    checkpoint_rows::{
        insert_checkpoint, load_at_seq, load_latest, load_run_slice, validate_projection,
    },
    checkpoint_snapshot::{SnapshotIdentity, build_snapshot},
    scheduler::{Event, add_object_ref, append_event},
};

impl SqliteStore {
    pub async fn create_runtime_checkpoint(
        &self,
        run_id: &str,
        now: i64,
    ) -> StorageResult<RuntimeCheckpointView> {
        let transaction = self.db.begin().await?;
        let slice = load_run_slice(&transaction, run_id).await?;
        if let Some(existing) = load_at_seq(&transaction, run_id, slice.through_seq).await? {
            transaction.commit().await?;
            return Ok(existing);
        }
        validate_projection(&transaction, run_id, slice.through_seq).await?;
        let snapshot = build_snapshot(
            &transaction,
            SnapshotIdentity {
                run_id,
                graph_revision_id: &slice.graph_revision_id,
                context_id: &slice.context_id,
                branch_id: &slice.branch_id,
                head_commit_id: &slice.head_commit_id,
                status: &slice.status,
                control_epoch: slice.control_epoch,
                through_seq: slice.through_seq,
            },
        )
        .await?;
        let checksum = canonical::hash(&snapshot)?;
        let snapshot_ref =
            put_inline_object(&transaction, &canonical::to_vec(&snapshot)?, now).await?;
        let checkpoint = RuntimeCheckpointView {
            id: new_id("runcheckpoint"),
            run_id: run_id.into(),
            context_branch_id: slice.branch_id,
            through_seq: slice.through_seq,
            graph_revision_id: slice.graph_revision_id,
            head_commit_id: slice.head_commit_id,
            snapshot_ref,
            effect_watermark: slice.effect_watermark,
            schema_version: 1,
            checksum,
            created_at: now,
        };
        insert_checkpoint(&transaction, &checkpoint).await?;
        add_object_ref(
            &transaction,
            &checkpoint.snapshot_ref,
            "runtime_checkpoint",
            &checkpoint.id,
            "snapshot",
            now,
        )
        .await?;
        append_event(&transaction, Event {
            run_id, event_type: "checkpoint.created", importance: "critical",
            node_instance_id: None, attempt_id: None,
            payload: json!({"schemaVersion":1,"checkpointId":checkpoint.id,"throughSeq":checkpoint.through_seq,"checksum":checkpoint.checksum}),
            now,
        }).await?;
        transaction.commit().await?;
        Ok(checkpoint)
    }

    pub async fn recover_runtime_runs(&self) -> StorageResult<Vec<RuntimeRecoveryView>> {
        let rows = self.db.query_all_raw(sql(
            "SELECT id FROM graph_runs WHERE status IN ('created','running','waiting','interrupting','interrupted') ORDER BY created_at,id",
            vec![],
        )).await?;
        let mut recovered = Vec::with_capacity(rows.len());
        for row in rows {
            let run_id: String = row.try_get("", "id")?;
            let checkpoint = match self.load_latest_runtime_checkpoint(&run_id).await? {
                Some(value) => value,
                None => {
                    self.create_runtime_checkpoint(&run_id, crate::graph::helpers::now_ms())
                        .await?
                }
            };
            recovered.push(self.reconcile_runtime_checkpoint(&checkpoint).await?);
        }
        Ok(recovered)
    }

    pub async fn load_latest_runtime_checkpoint(
        &self,
        run_id: &str,
    ) -> StorageResult<Option<RuntimeCheckpointView>> {
        load_latest(&self.db, run_id).await
    }

    async fn reconcile_runtime_checkpoint(
        &self,
        checkpoint: &RuntimeCheckpointView,
    ) -> StorageResult<RuntimeRecoveryView> {
        let snapshot: Value = load_object_json(&self.db, &checkpoint.snapshot_ref).await?;
        validate_snapshot(checkpoint, &snapshot)?;
        let rows = self.db.query_all_raw(sql(
            "SELECT seq,event_type,schema_version FROM run_events WHERE run_id = ? AND seq > ? ORDER BY seq",
            vec![checkpoint.run_id.clone().into(), (checkpoint.through_seq as i64).into()],
        )).await?;
        let mut through = checkpoint.through_seq;
        for row in &rows {
            let seq = u64::try_from(row.try_get::<i64>("", "seq")?)
                .map_err(|_| StorageError::Integrity("invalid runtime event sequence".into()))?;
            if seq <= through || row.try_get::<i64>("", "schema_version")? != 1 {
                return Err(StorageError::Integrity(
                    "runtime checkpoint journal tail is invalid".into(),
                ));
            }
            through = seq;
        }
        validate_projection(&self.db, &checkpoint.run_id, through).await?;
        Ok(RuntimeRecoveryView {
            run_id: checkpoint.run_id.clone(),
            checkpoint_id: checkpoint.id.clone(),
            replayed_event_count: rows.len() as u64,
            recovered_through_seq: through,
            projection_consistent: true,
        })
    }
}

fn validate_snapshot(checkpoint: &RuntimeCheckpointView, snapshot: &Value) -> StorageResult<()> {
    if checkpoint.schema_version != 1
        || canonical::hash(snapshot)? != checkpoint.checksum
        || snapshot.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || snapshot.get("throughSeq").and_then(Value::as_u64) != Some(checkpoint.through_seq)
        || snapshot.pointer("/run/id").and_then(Value::as_str) != Some(checkpoint.run_id.as_str())
        || snapshot
            .pointer("/run/graphRevisionId")
            .and_then(Value::as_str)
            != Some(checkpoint.graph_revision_id.as_str())
        || snapshot.pointer("/run/branchId").and_then(Value::as_str)
            != Some(checkpoint.context_branch_id.as_str())
        || snapshot
            .pointer("/run/headCommitId")
            .and_then(Value::as_str)
            != Some(checkpoint.head_commit_id.as_str())
    {
        return Err(StorageError::Integrity(
            "runtime checkpoint failed identity or checksum validation".into(),
        ));
    }
    Ok(())
}
