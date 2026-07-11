use sea_orm::ConnectionTrait;
use zhuangsheng_core::application::context::{ContextCommitView, WorkingContextView};
use zhuangsheng_core::state::{ActorKind, ActorRef};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

pub(crate) async fn load_context<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    branch_id: &str,
) -> StorageResult<WorkingContextView> {
    let row = connection.query_one(sql(
        "SELECT b.head_commit_id, p.projection_json, p.projection_object_id FROM context_branches b JOIN materialized_projections p ON p.aggregate_kind = 'working_context' AND p.aggregate_id = b.context_id AND p.lineage_key = b.id WHERE b.context_id = ? AND b.id = ?",
        vec![context_id.into(), branch_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound {
        kind: "context_branch",
        id: branch_id.into(),
    })?;
    let projection_json: Option<String> = row.try_get("", "projection_json")?;
    let value = if let Some(value) = projection_json {
        serde_json::from_str(&value).map_err(|error| StorageError::Integrity(error.to_string()))?
    } else {
        let object_id: String = row.try_get("", "projection_object_id")?;
        load_object_json(connection, &object_id).await?
    };
    Ok(WorkingContextView {
        context_id: context_id.into(),
        branch_id: branch_id.into(),
        head_commit_id: row.try_get("", "head_commit_id")?,
        value,
    })
}

pub(crate) async fn load_commit<C: ConnectionTrait>(
    connection: &C,
    commit_id: &str,
) -> StorageResult<ContextCommitView> {
    let row = connection.query_one(sql(
        "SELECT id, aggregate_id, lineage_key, sequence_no, operation_id, patch_object_id, schema_version, policy_version, author_kind, author_id, origin_run_id, origin_node_instance_id, created_at FROM version_commits WHERE id = ? AND aggregate_kind = 'working_context'",
        vec![commit_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound {
        kind: "context_commit",
        id: commit_id.into(),
    })?;
    let parents = connection
        .query_all(sql(
            "SELECT parent_commit_id FROM commit_parents WHERE commit_id = ? ORDER BY parent_order",
            vec![commit_id.into()],
        ))
        .await?;
    Ok(ContextCommitView {
        id: row.try_get("", "id")?,
        context_id: row.try_get("", "aggregate_id")?,
        branch_id: row.try_get("", "lineage_key")?,
        sequence_no: to_u64(row.try_get("", "sequence_no")?, "sequence")?,
        operation_id: row.try_get("", "operation_id")?,
        parent_commit_ids: parents
            .iter()
            .map(|parent| parent.try_get("", "parent_commit_id"))
            .collect::<Result<_, _>>()?,
        patch_ref: row.try_get("", "patch_object_id")?,
        schema_version: to_u32(row.try_get("", "schema_version")?, "schema version")?,
        policy_version: to_u32(row.try_get("", "policy_version")?, "policy version")?,
        author: ActorRef {
            kind: parse_actor(&row.try_get::<String>("", "author_kind")?)?,
            id: row.try_get("", "author_id")?,
        },
        origin_run_id: row.try_get("", "origin_run_id")?,
        origin_node_instance_id: row.try_get("", "origin_node_instance_id")?,
        created_at: row.try_get("", "created_at")?,
    })
}

fn parse_actor(value: &str) -> StorageResult<ActorKind> {
    Ok(match value {
        "user" => ActorKind::User,
        "system" => ActorKind::System,
        "node" => ActorKind::Node,
        "tool" => ActorKind::Tool,
        "application" => ActorKind::Application,
        _ => return Err(StorageError::Integrity("invalid commit author kind".into())),
    })
}

fn to_u64(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value).map_err(|_| StorageError::Integrity(format!("invalid {field}")))
}

fn to_u32(value: i64, field: &str) -> StorageResult<u32> {
    u32::try_from(value).map_err(|_| StorageError::Integrity(format!("invalid {field}")))
}
