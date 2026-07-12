use sea_orm::ConnectionTrait;
use zhuangsheng_core::context_merge::{ExplicitMergeResolution, ExplicitMergeSelection};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) async fn validate_selections<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    selections: &[ExplicitMergeSelection],
) -> StorageResult<()> {
    for selection in selections {
        let ExplicitMergeResolution::ArtifactRef { artifact_ref } = &selection.resolution else {
            continue;
        };
        artifact_ref
            .validate()
            .map_err(|message| StorageError::InvalidArgument(message.into()))?;
        let row = connection.query_one_raw(sql(
            "SELECT a.context_id, a.media_type, a.status, c.content_hash, c.byte_size, c.lifecycle FROM artifacts a JOIN content_objects c ON c.id = a.content_object_id WHERE a.id = ?",
            vec![artifact_ref.artifact_id.clone().into()],
        )).await?;
        let Some(row) = row else {
            return Err(unreachable_artifact());
        };
        let artifact_context: Option<String> = row.try_get("", "context_id")?;
        let byte_size = i64::try_from(artifact_ref.byte_size)
            .map_err(|_| StorageError::InvalidArgument("artifact byte size is invalid".into()))?;
        if artifact_context.as_deref() != Some(context_id)
            || row.try_get::<String>("", "media_type")? != artifact_ref.media_type
            || row.try_get::<String>("", "status")? != "active"
            || row.try_get::<String>("", "content_hash")? != artifact_ref.content_hash
            || row.try_get::<i64>("", "byte_size")? != byte_size
            || row.try_get::<String>("", "lifecycle")? != "live"
        {
            return Err(unreachable_artifact());
        }
    }
    Ok(())
}

fn unreachable_artifact() -> StorageError {
    StorageError::InvalidArgument("artifact resolution is not reachable from this context".into())
}
