use sea_orm::ConnectionTrait;
use zhuangsheng_core::runtime::{
    WaitBlockerKind, WaitBlockerStatus, WaitBlockerView, WaitKind, WaitStatus, WaitView,
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

impl SqliteStore {
    pub async fn list_open_waits(&self, run_id: &str) -> StorageResult<Vec<WaitView>> {
        self.get_run(run_id).await?;
        let rows = self
            .db
            .query_all_raw(sql(
                "SELECT id, run_id, node_instance_id, node_attempt_id, kind, request_object_id, response_schema_object_id, response_schema_compilation_object_id, correlation_key, deadline_at, status, accepted_delivery_id, created_at, resolved_at FROM node_waits WHERE run_id = ? AND status = 'open' ORDER BY created_at, id",
                vec![run_id.into()],
            ))
            .await?;
        let mut waits = Vec::with_capacity(rows.len());
        for row in rows {
            let wait_id: String = row.try_get("", "id")?;
            let request_ref: String = row.try_get("", "request_object_id")?;
            waits.push(WaitView {
                id: wait_id.clone(),
                run_id: row.try_get("", "run_id")?,
                node_instance_id: row.try_get("", "node_instance_id")?,
                attempt_id: row.try_get("", "node_attempt_id")?,
                kind: parse_kind(&row.try_get::<String>("", "kind")?)?,
                request: load_object_json(&self.db, &request_ref).await?,
                request_ref,
                response_schema: load_optional_object(
                    &self.db,
                    row.try_get("", "response_schema_object_id")?,
                )
                .await?,
                response_schema_compilation: load_optional_object(
                    &self.db,
                    row.try_get("", "response_schema_compilation_object_id")?,
                )
                .await?,
                correlation_key: row.try_get("", "correlation_key")?,
                deadline_at: row.try_get("", "deadline_at")?,
                status: parse_status(&row.try_get::<String>("", "status")?)?,
                blockers: load_blockers(&self.db, &wait_id).await?,
                accepted_delivery_id: row.try_get("", "accepted_delivery_id")?,
                created_at: row.try_get("", "created_at")?,
                resolved_at: row.try_get("", "resolved_at")?,
            });
        }
        Ok(waits)
    }
}

async fn load_optional_object<C, T>(
    connection: &C,
    object_id: Option<String>,
) -> StorageResult<Option<T>>
where
    C: ConnectionTrait,
    T: serde::de::DeserializeOwned,
{
    match object_id {
        Some(object_id) => Ok(Some(load_object_json(connection, &object_id).await?)),
        None => Ok(None),
    }
}

async fn load_blockers<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
) -> StorageResult<Vec<WaitBlockerView>> {
    connection
        .query_all_raw(sql(
            "SELECT blocker_kind, blocker_id, blocker_order, status, decision_object_id FROM wait_blockers WHERE wait_id = ? ORDER BY blocker_order",
            vec![wait_id.into()],
        ))
        .await?
        .into_iter()
        .map(|row| {
            Ok(WaitBlockerView {
                kind: parse_blocker_kind(&row.try_get::<String>("", "blocker_kind")?)?,
                id: row.try_get("", "blocker_id")?,
                order: u64::try_from(row.try_get::<i64>("", "blocker_order")?)
                    .map_err(|_| StorageError::Integrity("negative blocker order".into()))?,
                status: parse_blocker_status(&row.try_get::<String>("", "status")?)?,
                decision_ref: row.try_get("", "decision_object_id")?,
            })
        })
        .collect()
}

fn parse_kind(value: &str) -> StorageResult<WaitKind> {
    match value {
        "human_response" => Ok(WaitKind::HumanResponse),
        "approval" => Ok(WaitKind::Approval),
        "webhook" => Ok(WaitKind::Webhook),
        "timer" => Ok(WaitKind::Timer),
        "external_job" => Ok(WaitKind::ExternalJob),
        "effect_resolution" => Ok(WaitKind::EffectResolution),
        "secret_store_unlocked" => Ok(WaitKind::SecretStoreUnlocked),
        _ => Err(StorageError::Integrity("unknown wait kind".into())),
    }
}

fn parse_status(value: &str) -> StorageResult<WaitStatus> {
    match value {
        "open" => Ok(WaitStatus::Open),
        "resolved" => Ok(WaitStatus::Resolved),
        "expired" => Ok(WaitStatus::Expired),
        "cancelled" => Ok(WaitStatus::Cancelled),
        _ => Err(StorageError::Integrity("unknown wait status".into())),
    }
}

fn parse_blocker_kind(value: &str) -> StorageResult<WaitBlockerKind> {
    match value {
        "tool_call" => Ok(WaitBlockerKind::ToolCall),
        "memory_proposal" => Ok(WaitBlockerKind::MemoryProposal),
        "effect" => Ok(WaitBlockerKind::Effect),
        _ => Err(StorageError::Integrity("unknown wait blocker kind".into())),
    }
}

fn parse_blocker_status(value: &str) -> StorageResult<WaitBlockerStatus> {
    match value {
        "open" => Ok(WaitBlockerStatus::Open),
        "satisfied" => Ok(WaitBlockerStatus::Satisfied),
        "rejected" => Ok(WaitBlockerStatus::Rejected),
        "aborted" => Ok(WaitBlockerStatus::Aborted),
        _ => Err(StorageError::Integrity(
            "unknown wait blocker status".into(),
        )),
    }
}
