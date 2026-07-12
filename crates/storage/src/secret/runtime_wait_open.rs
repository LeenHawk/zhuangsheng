use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{application::secret::ResolveRuntimeSecretCommand, canonical};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{apply::load_revision, helpers::*},
    runtime::{Event, add_object_ref, append_event, compute_llm_read_set_digest},
};

use super::runtime_wait::SecretUnlockContinuation;

impl SqliteStore {
    pub(super) async fn validate_runtime_secret_owner(
        &self,
        command: &ResolveRuntimeSecretCommand,
        now: i64,
    ) -> StorageResult<()> {
        validate_command(command)?;
        let transaction = self.db.begin().await?;
        let owner = load_owner(&transaction, command).await?;
        validate_owner(&owner, command, now)?;
        let digest = compute_llm_read_set_digest(&transaction, &command.attempt_id).await?;
        if digest != command.read_set_digest {
            return Err(StorageError::Conflict("secret_wait_read_set_changed"));
        }
        transaction.commit().await?;
        Ok(())
    }

    pub(super) async fn open_secret_unlock_wait(
        &self,
        command: &ResolveRuntimeSecretCommand,
        now: i64,
    ) -> StorageResult<String> {
        validate_command(command)?;
        let transaction = self.db.begin().await?;
        let owner = load_owner(&transaction, command).await?;
        validate_owner(&owner, command, now)?;
        if transaction.query_one(sql(
            "SELECT 1 AS present FROM node_waits WHERE node_instance_id = ? AND status = 'open'",
            vec![command.node_instance_id.clone().into()],
        )).await?.is_some() {
            return Err(StorageError::Conflict("node_instance_open_wait"));
        }
        let read_set_digest =
            compute_llm_read_set_digest(&transaction, &command.attempt_id).await?;
        if read_set_digest != command.read_set_digest {
            return Err(StorageError::Conflict("secret_wait_read_set_changed"));
        }
        let revision = load_revision(&transaction, &owner.graph_revision_id).await?;
        let max_open_waits =
            i64::try_from(revision.definition.limits.max_open_waits).map_err(|_| {
                StorageError::InvalidArgument("run open wait limit is too large".into())
            })?;
        if owner.open_waits >= max_open_waits {
            return Err(StorageError::InvalidArgument(
                "run open wait limit exceeded".into(),
            ));
        }
        if owner.attempt_count >= revision.definition.limits.max_attempts_per_activation {
            return Err(StorageError::InvalidArgument(
                "node attempt limit prevents secret wait resume".into(),
            ));
        }
        let wait_id = new_id("wait");
        let request_ref = put_inline_object(
            &transaction,
            &canonical::to_vec(&json!({
                "schemaVersion":1,
                "kind":"secret_store_unlocked",
                "reason":"provider_credential_required",
                "channelId":command.channel_id,
            }))?,
            now,
        )
        .await?;
        let continuation = SecretUnlockContinuation {
            schema_version: 1,
            node_instance_id: command.node_instance_id.clone(),
            source_attempt_id: command.attempt_id.clone(),
            execution_snapshot_ref: owner.execution_snapshot_ref.clone(),
            read_set_digest,
        };
        let continuation_ref =
            put_inline_object(&transaction, &canonical::to_vec(&continuation)?, now).await?;
        transaction.execute(sql(
            "INSERT INTO node_waits (id, run_id, node_instance_id, node_attempt_id, kind, correlation_key, request_object_id, continuation_object_id, on_timeout, status, created_at) VALUES (?, ?, ?, ?, 'secret_store_unlocked', ?, ?, ?, 'fail', 'open', ?)",
            vec![
                wait_id.clone().into(), command.run_id.clone().into(),
                command.node_instance_id.clone().into(), command.attempt_id.clone().into(),
                format!("secret-store:{}", command.node_instance_id).into(),
                request_ref.clone().into(), continuation_ref.clone().into(), now.into(),
            ],
        )).await?;
        transition_owner(
            &transaction,
            command,
            &continuation_ref,
            owner.run_status == "running",
            now,
        )
        .await?;
        for (object_id, role) in [
            (&request_ref, "request"),
            (&continuation_ref, "continuation"),
        ] {
            add_object_ref(&transaction, object_id, "node_wait", &wait_id, role, now).await?;
        }
        append_event(
            &transaction,
            Event {
                run_id: &command.run_id,
                event_type: "node.wait.secret_store_required",
                importance: "critical",
                node_instance_id: Some(&command.node_instance_id),
                attempt_id: Some(&command.attempt_id),
                payload: json!({"schemaVersion":1,"waitId":wait_id,"channelId":command.channel_id}),
                now,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(wait_id)
    }
}

struct WaitOwner {
    attempt_status: String,
    worker_id: Option<String>,
    lease_fence: i64,
    attempt_epoch: i64,
    lease_until: Option<i64>,
    attempt_deadline: Option<i64>,
    instance_status: String,
    graph_revision_id: String,
    execution_snapshot_ref: String,
    run_status: String,
    control_epoch: i64,
    drain_epoch: Option<i64>,
    run_deadline: i64,
    open_waits: i64,
    attempt_count: u64,
}

async fn load_owner<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveRuntimeSecretCommand,
) -> StorageResult<WaitOwner> {
    let row = connection.query_one(sql(
        "SELECT a.status AS attempt_status, a.worker_id, a.lease_fence, a.run_control_epoch AS attempt_epoch, a.lease_until, a.deadline_at AS attempt_deadline, ni.status AS instance_status, ni.graph_revision_id, ni.execution_snapshot_object_id, r.status AS run_status, r.control_epoch, r.drain_epoch, r.deadline_at AS run_deadline, c.open_waits, (SELECT COUNT(*) FROM node_attempts all_attempts WHERE all_attempts.node_instance_id = ni.id) AS attempt_count FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id JOIN run_execution_counters c ON c.run_id = r.id WHERE a.id = ? AND ni.id = ? AND r.id = ?",
        vec![command.attempt_id.clone().into(), command.node_instance_id.clone().into(), command.run_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::Conflict("secret_wait_owner"))?;
    Ok(WaitOwner {
        attempt_status: row.try_get("", "attempt_status")?,
        worker_id: row.try_get("", "worker_id")?,
        lease_fence: row.try_get("", "lease_fence")?,
        attempt_epoch: row.try_get("", "attempt_epoch")?,
        lease_until: row.try_get("", "lease_until")?,
        attempt_deadline: row.try_get("", "attempt_deadline")?,
        instance_status: row.try_get("", "instance_status")?,
        graph_revision_id: row.try_get("", "graph_revision_id")?,
        execution_snapshot_ref: row
            .try_get::<Option<String>>("", "execution_snapshot_object_id")?
            .ok_or_else(|| StorageError::Conflict("secret_wait_snapshot_missing"))?,
        run_status: row.try_get("", "run_status")?,
        control_epoch: row.try_get("", "control_epoch")?,
        drain_epoch: row.try_get("", "drain_epoch")?,
        run_deadline: row.try_get("", "run_deadline")?,
        open_waits: row.try_get("", "open_waits")?,
        attempt_count: u64::try_from(row.try_get::<i64>("", "attempt_count")?)
            .map_err(|_| StorageError::Integrity("invalid node attempt count".into()))?,
    })
}

fn validate_owner(
    owner: &WaitOwner,
    command: &ResolveRuntimeSecretCommand,
    now: i64,
) -> StorageResult<()> {
    let fence = i64::try_from(command.lease_fence)
        .map_err(|_| StorageError::Conflict("secret_wait_fence"))?;
    let epoch = i64::try_from(command.run_control_epoch)
        .map_err(|_| StorageError::Conflict("secret_wait_fence"))?;
    let lifecycle = (owner.run_status == "running" && owner.control_epoch == epoch)
        || (owner.run_status == "interrupting" && owner.drain_epoch == Some(epoch));
    if owner.attempt_status != "running"
        || owner.instance_status != "running"
        || owner.worker_id.as_deref() != Some(&command.worker_id)
        || owner.lease_fence != fence
        || owner.attempt_epoch != epoch
        || !lifecycle
        || owner.lease_until.is_none_or(|v| now >= v)
        || owner.attempt_deadline.is_none_or(|v| now >= v)
        || now >= owner.run_deadline
    {
        return Err(StorageError::Conflict("secret_wait_fence"));
    }
    Ok(())
}

async fn transition_owner<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveRuntimeSecretCommand,
    continuation_ref: &str,
    may_wait_run: bool,
    now: i64,
) -> StorageResult<()> {
    let fence = i64::try_from(command.lease_fence)
        .map_err(|_| StorageError::Conflict("secret_wait_fence"))?;
    let attempt = connection.execute(sql(
        "UPDATE node_attempts SET status = 'waiting', continuation_object_id = ?, worker_id = NULL, lease_until = NULL, finished_at = ? WHERE id = ? AND node_instance_id = ? AND status = 'running' AND worker_id = ? AND lease_fence = ?",
        vec![continuation_ref.into(), now.into(), command.attempt_id.clone().into(), command.node_instance_id.clone().into(), command.worker_id.clone().into(), fence.into()],
    )).await?;
    let instance = connection.execute(sql(
        "UPDATE node_instances SET status = 'waiting', updated_at = ? WHERE id = ? AND status = 'running'",
        vec![now.into(), command.node_instance_id.clone().into()],
    )).await?;
    if attempt.rows_affected() != 1 || instance.rows_affected() != 1 {
        return Err(StorageError::Conflict("secret_wait_owner_status"));
    }
    connection.execute(sql("UPDATE runtime_timers SET status = 'cancelled' WHERE node_attempt_id = ? AND kind = 'attempt_deadline' AND status = 'pending'", vec![command.attempt_id.clone().into()])).await?;
    if connection.execute(sql(
        "UPDATE scheduler_wakeups SET status = 'done', claimed_by = NULL, lease_until = NULL WHERE id = ? AND run_id = ? AND status = 'claimed' AND claimed_by = ?",
        vec![command.wakeup_id.clone().into(), command.run_id.clone().into(), command.worker_id.clone().into()],
    )).await?.rows_affected() != 1 { return Err(StorageError::Conflict("secret_wait_wakeup")); }
    connection
        .execute(sql(
            "UPDATE run_execution_counters SET open_waits = open_waits + 1 WHERE run_id = ?",
            vec![command.run_id.clone().into()],
        ))
        .await?;
    let has_active_instance = connection.query_one(sql(
        "SELECT 1 AS present FROM node_instances WHERE run_id = ? AND status IN ('ready','running') LIMIT 1",
        vec![command.run_id.clone().into()],
    )).await?.is_some();
    let has_dispatch_wakeup = connection.query_one(sql(
        "SELECT 1 AS present FROM scheduler_wakeups WHERE run_id = ? AND kind IN ('node_maybe_ready','attempt_ready') AND status IN ('pending','claimed') LIMIT 1",
        vec![command.run_id.clone().into()],
    )).await?.is_some();
    if may_wait_run && !has_active_instance && !has_dispatch_wakeup {
        connection.execute(sql("UPDATE graph_runs SET status = 'waiting', updated_at = ? WHERE id = ? AND status = 'running'", vec![now.into(), command.run_id.clone().into()])).await?;
    }
    Ok(())
}

fn validate_command(command: &ResolveRuntimeSecretCommand) -> StorageResult<()> {
    if [
        &command.run_id,
        &command.node_instance_id,
        &command.attempt_id,
        &command.wakeup_id,
        &command.worker_id,
        &command.channel_id,
        &command.read_set_digest,
    ]
    .iter()
    .any(|value| value.is_empty() || value.len() > 256)
    {
        return Err(StorageError::InvalidArgument(
            "invalid runtime secret wait command".into(),
        ));
    }
    Ok(())
}
