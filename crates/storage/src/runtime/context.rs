use sea_orm::ConnectionTrait;

use crate::{StorageError, StorageResult, graph::helpers::*};

pub(super) struct ContextBinding {
    pub context_id: String,
    pub branch_id: String,
    pub input_commit_id: String,
}

pub(super) async fn create_temporary_context<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<ContextBinding> {
    let context_id = new_id("context");
    let branch_id = new_id("branch");
    let commit_id = new_id("commit");
    let snapshot = zhuangsheng_core::canonical::to_vec(&serde_json::json!({}))?;
    let snapshot_id = put_inline_object(connection, &snapshot, now).await?;
    connection
        .execute_raw(sql(
            "INSERT INTO contexts (id, kind, status, created_at, updated_at) VALUES (?, 'temporary', 'active', ?, ?)",
            vec![context_id.clone().into(), now.into(), now.into()],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "INSERT INTO version_commits (id, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, initial_snapshot_object_id, schema_version, policy_version, author_kind, created_at) VALUES (?, 'working_context', ?, ?, 1, ?, ?, 1, 1, 'system', ?)",
            vec![commit_id.clone().into(), context_id.clone().into(), branch_id.clone().into(), format!("temporary-context-root:{run_id}").into(), snapshot_id.clone().into(), now.into()],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "INSERT INTO context_branches (id, context_id, fork_commit_id, head_commit_id, creation_operation_id, status, pinned, audit_hold, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 'active', 0, 0, ?, ?)",
            vec![branch_id.clone().into(), context_id.clone().into(), commit_id.clone().into(), commit_id.clone().into(), format!("temporary-context-root-branch:{run_id}").into(), now.into(), now.into()],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "INSERT INTO materialized_projections (aggregate_kind, aggregate_id, lineage_key, head_commit_id, projection_json, schema_version, updated_at) VALUES ('working_context', ?, ?, ?, '{}', 1, ?)",
            vec![context_id.clone().into(), branch_id.clone().into(), commit_id.clone().into(), now.into()],
        ))
        .await?;
    connection
        .execute_raw(sql(
            "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'version_commit', ?, 'initial_snapshot', ?)",
            vec![snapshot_id.into(), commit_id.clone().into(), now.into()],
        ))
        .await?;
    Ok(ContextBinding {
        context_id,
        branch_id,
        input_commit_id: commit_id,
    })
}

pub(super) async fn bind_existing_context<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    branch_id: &str,
    expected_head: &str,
) -> StorageResult<ContextBinding> {
    let matched = connection
        .execute_raw(sql(
            "UPDATE context_branches SET updated_at = updated_at WHERE context_id = ? AND id = ? AND head_commit_id = ? AND status = 'active'",
            vec![context_id.into(), branch_id.into(), expected_head.into()],
        ))
        .await?;
    if matched.rows_affected() != 1 {
        let exists = connection
            .query_one_raw(sql(
                "SELECT 1 AS present FROM context_branches WHERE context_id = ? AND id = ?",
                vec![context_id.into(), branch_id.into()],
            ))
            .await?;
        return if exists.is_some() {
            Err(StorageError::Conflict("context_head"))
        } else {
            Err(StorageError::NotFound {
                kind: "context_branch",
                id: branch_id.into(),
            })
        };
    }
    Ok(ContextBinding {
        context_id: context_id.into(),
        branch_id: branch_id.into(),
        input_commit_id: expected_head.into(),
    })
}
