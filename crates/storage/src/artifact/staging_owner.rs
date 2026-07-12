use sea_orm::ConnectionTrait;
use zhuangsheng_core::application::artifact::CreateArtifactStagingCommand;

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) async fn validate_owner<C: ConnectionTrait>(
    connection: &C,
    command: &CreateArtifactStagingCommand,
) -> StorageResult<Option<String>> {
    if let Some(context_id) = &command.context_id {
        let row = connection
            .query_one_raw(sql(
                "SELECT status FROM contexts WHERE id = ?",
                vec![context_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::NotFound {
                kind: "context",
                id: context_id.clone(),
            })?;
        if row.try_get::<String>("", "status")? != "active" {
            return Err(StorageError::Conflict("artifact_context_inactive"));
        }
    }
    let Some(attempt_id) = &command.node_attempt_id else {
        return Ok(command.context_id.clone());
    };
    let row = connection.query_one_raw(sql(
        "SELECT a.node_instance_id, a.status AS attempt_status, r.context_id FROM node_attempts a JOIN node_instances ni ON ni.id = a.node_instance_id JOIN graph_runs r ON r.id = ni.run_id WHERE a.id = ?",
        vec![attempt_id.clone().into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "node_attempt", id: attempt_id.clone() })?;
    let node_instance_id: String = row.try_get("", "node_instance_id")?;
    let context_id: String = row.try_get("", "context_id")?;
    if row.try_get::<String>("", "attempt_status")? != "running" {
        return Err(StorageError::Conflict("artifact_staging_writer_inactive"));
    }
    if command
        .context_id
        .as_ref()
        .is_some_and(|id| id != &context_id)
    {
        return Err(StorageError::Conflict("artifact_context_binding"));
    }
    let Some(tool_call_id) = &command.tool_call_id else {
        return Ok(Some(context_id));
    };
    let tool = connection
        .query_one_raw(sql(
            "SELECT node_instance_id FROM tool_calls WHERE id = ?",
            vec![tool_call_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "tool_call",
            id: tool_call_id.clone(),
        })?;
    if tool.try_get::<String>("", "node_instance_id")? != node_instance_id {
        return Err(StorageError::Conflict("artifact_tool_binding"));
    }
    Ok(Some(context_id))
}
