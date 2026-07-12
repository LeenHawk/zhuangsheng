use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::canonical;

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, put_inline_object, sql},
    runtime::{
        Event, ResumeAttempt, add_object_ref, append_event, compute_llm_read_set_digest,
        create_resume_attempt, enqueue_wakeup,
    },
};

use super::runtime_wait::SecretUnlockContinuation;

impl SqliteStore {
    pub(super) async fn resolve_secret_unlock_waits(
        &self,
        session_id: &str,
        now: i64,
    ) -> StorageResult<()> {
        if session_id.is_empty() || session_id.len() > 256 {
            return Err(StorageError::InvalidArgument(
                "invalid secret unlock session".into(),
            ));
        }
        let transaction = self.db.begin().await?;
        let waits = load_open_waits(&transaction).await?;
        for wait in waits {
            resolve_one(&transaction, &wait, session_id, now).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

struct OpenSecretWait {
    wait_id: String,
    run_id: String,
    run_status: String,
    control_epoch: u64,
    node_instance_id: String,
    node_id: String,
    instance_status: String,
    execution_snapshot_ref: String,
    source_attempt_id: String,
    attempt_status: String,
    continuation_ref: String,
}

async fn load_open_waits<C: ConnectionTrait>(connection: &C) -> StorageResult<Vec<OpenSecretWait>> {
    connection.query_all(sql(
        "SELECT w.id AS wait_id, w.run_id, w.node_instance_id, w.node_attempt_id, w.continuation_object_id, r.status AS run_status, r.control_epoch, ni.node_id, ni.status AS instance_status, ni.execution_snapshot_object_id, a.status AS attempt_status FROM node_waits w JOIN graph_runs r ON r.id = w.run_id JOIN node_instances ni ON ni.id = w.node_instance_id JOIN node_attempts a ON a.id = w.node_attempt_id WHERE w.kind = 'secret_store_unlocked' AND w.status = 'open' ORDER BY w.created_at, w.id",
        vec![],
    )).await?.into_iter().map(|row| {
        Ok(OpenSecretWait {
            wait_id: row.try_get("", "wait_id")?, run_id: row.try_get("", "run_id")?,
            run_status: row.try_get("", "run_status")?,
            control_epoch: u64::try_from(row.try_get::<i64>("", "control_epoch")?)
                .map_err(|_| StorageError::Integrity("invalid run control epoch".into()))?,
            node_instance_id: row.try_get("", "node_instance_id")?, node_id: row.try_get("", "node_id")?,
            instance_status: row.try_get("", "instance_status")?,
            execution_snapshot_ref: row.try_get::<Option<String>>("", "execution_snapshot_object_id")?
                .ok_or_else(|| StorageError::Integrity("secret wait snapshot is missing".into()))?,
            source_attempt_id: row.try_get("", "node_attempt_id")?,
            attempt_status: row.try_get("", "attempt_status")?,
            continuation_ref: row.try_get("", "continuation_object_id")?,
        })
    }).collect()
}

async fn resolve_one<C: ConnectionTrait>(
    connection: &C,
    wait: &OpenSecretWait,
    session_id: &str,
    now: i64,
) -> StorageResult<()> {
    if !matches!(
        wait.run_status.as_str(),
        "running" | "waiting" | "interrupting" | "interrupted"
    ) || wait.instance_status != "waiting"
        || wait.attempt_status != "waiting"
    {
        return Err(StorageError::Conflict("secret_wait_owner_status"));
    }
    let continuation: SecretUnlockContinuation =
        load_object_json(connection, &wait.continuation_ref).await?;
    if continuation.schema_version != 1
        || continuation.node_instance_id != wait.node_instance_id
        || continuation.source_attempt_id != wait.source_attempt_id
        || continuation.execution_snapshot_ref != wait.execution_snapshot_ref
    {
        return Err(StorageError::Integrity(
            "secret wait continuation is incompatible".into(),
        ));
    }
    let delivery_id = format!("unlock:{session_id}");
    let idempotency_key = format!("wait:{}:{delivery_id}:resume", wait.wait_id);
    let resume_attempt_id = create_resume_attempt(
        connection,
        ResumeAttempt {
            node_instance_id: &wait.node_instance_id,
            source_attempt_id: &wait.source_attempt_id,
            run_id: &wait.run_id,
            control_epoch: wait.control_epoch,
            idempotency_key: &idempotency_key,
        },
        now,
    )
    .await?;
    let copied_digest = compute_llm_read_set_digest(connection, &resume_attempt_id).await?;
    if copied_digest != continuation.read_set_digest {
        return Err(StorageError::Integrity(
            "secret wait read set changed during resume".into(),
        ));
    }
    let response = json!({
        "schemaVersion":1,
        "kind":"secret_store_unlocked",
        "sessionId":session_id,
    });
    let response_ref = put_inline_object(connection, &canonical::to_vec(&response)?, now).await?;
    if connection.execute(sql(
        "UPDATE node_waits SET status = 'resolved', response_object_id = ?, accepted_delivery_id = ?, resolved_at = ? WHERE id = ? AND status = 'open'",
        vec![response_ref.clone().into(), delivery_id.clone().into(), now.into(), wait.wait_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("secret_wait_resolution"));
    }
    connection.execute(sql(
        "INSERT INTO wait_deliveries (wait_id, delivery_id, payload_digest, result_object_id, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![wait.wait_id.clone().into(), delivery_id.clone().into(), canonical::hash(&response)?.into(), response_ref.clone().into(), now.into()],
    )).await?;
    if connection.execute(sql(
        "UPDATE node_instances SET status = 'ready', updated_at = ? WHERE id = ? AND status = 'waiting'",
        vec![now.into(), wait.node_instance_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("secret_wait_owner_status"));
    }
    if connection.execute(sql(
        "UPDATE run_execution_counters SET open_waits = open_waits - 1 WHERE run_id = ? AND open_waits > 0",
        vec![wait.run_id.clone().into()],
    )).await?.rows_affected() != 1 {
        return Err(StorageError::Integrity("secret wait counter underflow".into()));
    }
    if wait.run_status == "waiting" {
        connection.execute(sql(
            "UPDATE graph_runs SET status = 'running', updated_at = ? WHERE id = ? AND status = 'waiting'",
            vec![now.into(), wait.run_id.clone().into()],
        )).await?;
    }
    for (owner_kind, owner_id, role) in [
        ("node_wait", wait.wait_id.as_str(), "response"),
        ("wait_delivery", idempotency_key.as_str(), "result"),
    ] {
        add_object_ref(connection, &response_ref, owner_kind, owner_id, role, now).await?;
    }
    let seq = append_event(
        connection,
        Event {
            run_id: &wait.run_id,
            event_type: "node.wait.secret_store_resolved",
            importance: "critical",
            node_instance_id: Some(&wait.node_instance_id),
            attempt_id: Some(&wait.source_attempt_id),
            payload: json!({
                "schemaVersion":1,"waitId":wait.wait_id,"deliveryId":delivery_id,
                "resumeAttemptId":resume_attempt_id,
            }),
            now,
        },
    )
    .await?;
    if matches!(wait.run_status.as_str(), "running" | "waiting") {
        enqueue_wakeup(
            connection,
            &wait.run_id,
            Some(&wait.node_id),
            "attempt_ready",
            seq,
            &format!("wait-resume:{resume_attempt_id}"),
            now,
        )
        .await?;
    }
    Ok(())
}
