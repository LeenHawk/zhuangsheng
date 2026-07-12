use std::collections::HashSet;

use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::context::{CommitContextPatchCommand, ContextCommitView},
    canonical,
    state::{
        ActorKind, AggregateKind, JsonPatchOp, StatePatch, apply_patch, patches_conflict,
        validate_patch,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, now_ms, put_inline_object, sql},
};

use super::query::{load_commit, load_context};

const MAX_HISTORY_COMMITS: usize = 10_000;
const MAX_PATCH_BYTES: usize = 1024 * 1024;
const MAX_PROJECTION_BYTES: usize = 16 * 1024 * 1024;

impl SqliteStore {
    pub async fn commit_context_patch(
        &self,
        command: CommitContextPatchCommand,
    ) -> StorageResult<ContextCommitView> {
        let transaction = self.db.begin().await?;
        let result = commit_patch(&transaction, &command, now_ms()).await?;
        transaction.commit().await?;
        Ok(result)
    }
}

pub(crate) async fn commit_patch<C: ConnectionTrait>(
    connection: &C,
    command: &CommitContextPatchCommand,
    now: i64,
) -> StorageResult<ContextCommitView> {
    validate_patch(&command.patch)?;
    if command.patch.aggregate_kind != AggregateKind::WorkingContext {
        return Err(StorageError::InvalidArgument(
            "context commit requires working_context aggregate".into(),
        ));
    }
    validate_origin(command)?;
    let patch_bytes = canonical::to_vec(&command.patch)?;
    if patch_bytes.len() > MAX_PATCH_BYTES {
        return Err(StorageError::InvalidArgument(
            "StatePatch exceeds 1 MiB".into(),
        ));
    }
    let patch_hash = canonical::hash_bytes(&patch_bytes);
    if let Some(existing) = find_existing(connection, command, &patch_hash).await? {
        return Ok(existing);
    }

    let context = load_context(
        connection,
        &command.patch.aggregate_id,
        &command.patch.lineage_key,
    )
    .await?;
    let history = load_history(
        connection,
        &context.head_commit_id,
        &command.patch.base_commit_id,
    )
    .await?;
    if !history.base_found {
        return Err(StorageError::Conflict("context_patch_base"));
    }
    if history
        .since_base
        .iter()
        .any(|committed| patches_conflict(&command.patch, committed))
    {
        return Err(StorageError::Conflict("state_conflict"));
    }
    let effective = effective_patch(&command.patch, &history.append_ids);
    let next_value = if effective.ops.is_empty() {
        context.value.clone()
    } else {
        apply_patch(&context.value, &effective)?
    };
    let projection = canonical::to_string(&next_value)?;
    if projection.len() > MAX_PROJECTION_BYTES {
        return Err(StorageError::InvalidArgument(
            "WorkingContext projection exceeds 16 MiB".into(),
        ));
    }
    let patch_object_id = put_inline_object(connection, &patch_bytes, now).await?;
    let commit_id = new_id("commit");
    let sequence = next_sequence(connection, &context.head_commit_id).await?;
    insert_commit(
        connection,
        command,
        &commit_id,
        &patch_object_id,
        &context.head_commit_id,
        sequence,
        now,
    )
    .await?;
    advance_projection(
        connection,
        &command.patch,
        &context.head_commit_id,
        &commit_id,
        &projection,
        now,
    )
    .await?;
    add_commit_ref(connection, &patch_object_id, &commit_id, now).await?;
    bind_origin(connection, command, &commit_id, now).await?;
    append_domain_event(connection, command, &commit_id, sequence, now).await?;
    load_commit(connection, &commit_id).await
}

struct History {
    base_found: bool,
    since_base: Vec<StatePatch>,
    append_ids: HashSet<String>,
}

async fn load_history<C: ConnectionTrait>(
    connection: &C,
    head: &str,
    base: &str,
) -> StorageResult<History> {
    let mut current = Some(head.to_string());
    let mut before_base = true;
    let mut base_found = false;
    let mut since_base = Vec::new();
    let mut append_ids = HashSet::new();
    let mut visited = 0;
    while let Some(commit_id) = current {
        visited += 1;
        if visited > MAX_HISTORY_COMMITS {
            return Err(StorageError::Integrity(
                "context commit history exceeds traversal limit".into(),
            ));
        }
        if commit_id == base {
            base_found = true;
            before_base = false;
        }
        let row = connection.query_one_raw(sql(
            "SELECT v.patch_object_id, p.parent_commit_id FROM version_commits v LEFT JOIN commit_parents p ON p.commit_id = v.id AND p.parent_order = 0 WHERE v.id = ? AND v.aggregate_kind = 'working_context'",
            vec![commit_id.clone().into()],
        )).await?.ok_or_else(|| StorageError::Integrity("context commit ancestry is broken".into()))?;
        let patch_id: Option<String> = row.try_get("", "patch_object_id")?;
        if let Some(patch_id) = patch_id {
            let patch: StatePatch = load_object_json(connection, &patch_id).await?;
            for operation in &patch.ops {
                if let JsonPatchOp::Append { element_id, .. } = operation {
                    append_ids.insert(element_id.clone());
                }
            }
            if before_base {
                since_base.push(patch);
            }
        }
        current = row.try_get("", "parent_commit_id")?;
    }
    Ok(History {
        base_found,
        since_base,
        append_ids,
    })
}

pub(crate) fn effective_patch(patch: &StatePatch, append_ids: &HashSet<String>) -> StatePatch {
    let mut effective = patch.clone();
    effective.ops.retain(|operation| {
        !matches!(operation, JsonPatchOp::Append { element_id, .. } if append_ids.contains(element_id))
    });
    effective
}

async fn find_existing<C: ConnectionTrait>(
    connection: &C,
    command: &CommitContextPatchCommand,
    patch_hash: &str,
) -> StorageResult<Option<ContextCommitView>> {
    let patch = &command.patch;
    let row = connection.query_one_raw(sql(
        "SELECT v.id, v.origin_run_id, v.origin_node_instance_id, c.content_hash FROM version_commits v LEFT JOIN content_objects c ON c.id = v.patch_object_id WHERE v.aggregate_kind = 'working_context' AND v.aggregate_id = ? AND v.lineage_key = ? AND v.operation_id = ?",
        vec![patch.aggregate_id.clone().into(), patch.lineage_key.clone().into(), patch.operation_id.clone().into()],
    )).await?;
    let Some(row) = row else { return Ok(None) };
    let existing_hash: Option<String> = row.try_get("", "content_hash")?;
    let origin_run: Option<String> = row.try_get("", "origin_run_id")?;
    let origin_instance: Option<String> = row.try_get("", "origin_node_instance_id")?;
    if existing_hash.as_deref() != Some(patch_hash)
        || origin_run != command.origin_run_id
        || origin_instance != command.origin_node_instance_id
    {
        return Err(StorageError::IdempotencyConflict);
    }
    let id: String = row.try_get("", "id")?;
    Ok(Some(load_commit(connection, &id).await?))
}

async fn next_sequence<C: ConnectionTrait>(connection: &C, head: &str) -> StorageResult<i64> {
    let row = connection
        .query_one_raw(sql(
            "SELECT sequence_no FROM version_commits WHERE id = ?",
            vec![head.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("context head commit missing".into()))?;
    row.try_get::<i64>("", "sequence_no")?
        .checked_add(1)
        .ok_or_else(|| StorageError::Integrity("context sequence overflow".into()))
}

async fn insert_commit<C: ConnectionTrait>(
    connection: &C,
    command: &CommitContextPatchCommand,
    commit_id: &str,
    patch_object_id: &str,
    parent_commit_id: &str,
    sequence: i64,
    now: i64,
) -> StorageResult<()> {
    let patch = &command.patch;
    connection.execute_raw(sql(
        "INSERT INTO version_commits (id, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, patch_object_id, schema_version, policy_version, author_kind, author_id, origin_run_id, origin_node_instance_id, created_at) VALUES (?, 'working_context', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![commit_id.into(), patch.aggregate_id.clone().into(), patch.lineage_key.clone().into(), sequence.into(), patch.operation_id.clone().into(), patch_object_id.into(), i64::from(patch.schema_version).into(), i64::from(patch.policy_version).into(), actor_kind(patch.author.kind).into(), patch.author.id.clone().into(), command.origin_run_id.clone().into(), command.origin_node_instance_id.clone().into(), now.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO commit_parents (commit_id, parent_commit_id, parent_order) VALUES (?, ?, 0)",
        vec![commit_id.into(), parent_commit_id.into()],
    )).await?;
    Ok(())
}

async fn advance_projection<C: ConnectionTrait>(
    connection: &C,
    patch: &StatePatch,
    old_head: &str,
    new_head: &str,
    projection: &str,
    now: i64,
) -> StorageResult<()> {
    let branch = connection.execute_raw(sql(
        "UPDATE context_branches SET head_commit_id = ?, updated_at = ? WHERE context_id = ? AND id = ? AND head_commit_id = ? AND status = 'active'",
        vec![new_head.into(), now.into(), patch.aggregate_id.clone().into(), patch.lineage_key.clone().into(), old_head.into()],
    )).await?;
    if branch.rows_affected() != 1 {
        return Err(StorageError::Conflict("context_head"));
    }
    let materialized = connection.execute_raw(sql(
        "UPDATE materialized_projections SET head_commit_id = ?, projection_json = ?, projection_object_id = NULL, schema_version = ?, updated_at = ? WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND head_commit_id = ?",
        vec![new_head.into(), projection.into(), i64::from(patch.schema_version).into(), now.into(), patch.aggregate_id.clone().into(), patch.lineage_key.clone().into(), old_head.into()],
    )).await?;
    if materialized.rows_affected() != 1 {
        return Err(StorageError::Conflict("context_projection_head"));
    }
    Ok(())
}

async fn bind_origin<C: ConnectionTrait>(
    connection: &C,
    command: &CommitContextPatchCommand,
    commit_id: &str,
    now: i64,
) -> StorageResult<()> {
    let Some(run_id) = &command.origin_run_id else {
        return Ok(());
    };
    let updated = connection.execute_raw(sql(
        "UPDATE graph_runs SET output_commit_id = ?, updated_at = ? WHERE id = ? AND context_id = ? AND branch_id = ?",
        vec![commit_id.into(), now.into(), run_id.clone().into(), command.patch.aggregate_id.clone().into(), command.patch.lineage_key.clone().into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("run_context_binding"));
    }
    if let Some(instance_id) = &command.origin_node_instance_id {
        let row = connection.query_one_raw(sql(
            "SELECT COALESCE(MAX(output_order), 0) + 1 AS next_order FROM node_output_commits WHERE node_instance_id = ?",
            vec![instance_id.clone().into()],
        )).await?.expect("aggregate query returns a row");
        let order: i64 = row.try_get("", "next_order")?;
        let linked = connection.execute_raw(sql(
            "INSERT INTO node_output_commits (node_instance_id, commit_id, output_order) SELECT ?, ?, ? WHERE EXISTS (SELECT 1 FROM node_instances WHERE id = ? AND run_id = ?)",
            vec![instance_id.clone().into(), commit_id.into(), order.into(), instance_id.clone().into(), run_id.clone().into()],
        )).await?;
        if linked.rows_affected() != 1 {
            return Err(StorageError::Conflict("node_origin_binding"));
        }
    }
    Ok(())
}

async fn add_commit_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    commit_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'version_commit', ?, 'patch', ?)",
        vec![object_id.into(), commit_id.into(), now.into()],
    )).await?;
    Ok(())
}

async fn append_domain_event<C: ConnectionTrait>(
    connection: &C,
    command: &CommitContextPatchCommand,
    commit_id: &str,
    sequence: i64,
    now: i64,
) -> StorageResult<()> {
    let patch = &command.patch;
    connection.execute_raw(sql(
        "INSERT OR IGNORE INTO domain_event_counters (aggregate_kind, aggregate_id, lineage_key, next_seq) VALUES ('working_context', ?, ?, 1)",
        vec![patch.aggregate_id.clone().into(), patch.lineage_key.clone().into()],
    )).await?;
    let row = connection.query_one_raw(sql(
        "SELECT next_seq FROM domain_event_counters WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ?",
        vec![patch.aggregate_id.clone().into(), patch.lineage_key.clone().into()],
    )).await?.expect("domain event counter exists");
    let event_seq: i64 = row.try_get("", "next_seq")?;
    let updated = connection.execute_raw(sql(
        "UPDATE domain_event_counters SET next_seq = next_seq + 1 WHERE aggregate_kind = 'working_context' AND aggregate_id = ? AND lineage_key = ? AND next_seq = ?",
        vec![patch.aggregate_id.clone().into(), patch.lineage_key.clone().into(), event_seq.into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("domain_event_sequence"));
    }
    connection.execute_raw(sql(
        "INSERT INTO domain_events (id, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json, created_at) VALUES (?, 'working_context', ?, ?, ?, 'context.commit.created', 1, ?, ?)",
        vec![new_id("domain_event").into(), patch.aggregate_id.clone().into(), patch.lineage_key.clone().into(), event_seq.into(), canonical::to_string(&json!({"schemaVersion":1,"commitId":commit_id,"sequenceNo":sequence,"operationId":patch.operation_id}))?.into(), now.into()],
    )).await?;
    Ok(())
}

fn validate_origin(command: &CommitContextPatchCommand) -> StorageResult<()> {
    if command.origin_node_instance_id.is_some() && command.origin_run_id.is_none() {
        return Err(StorageError::InvalidArgument(
            "originNodeInstanceId requires originRunId".into(),
        ));
    }
    Ok(())
}

fn actor_kind(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::User => "user",
        ActorKind::System => "system",
        ActorKind::Node => "node",
        ActorKind::Tool => "tool",
        ActorKind::Application => "application",
    }
}
