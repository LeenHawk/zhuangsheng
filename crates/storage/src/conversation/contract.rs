use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    conversation::{ConversationRunSpec, validate_conversation_run_contract},
    graph::AppliedGraphDefinition,
};

use crate::{StorageError, StorageResult, graph::apply::load_revision};

pub(super) async fn validate_run_spec<C: ConnectionTrait>(
    connection: &C,
    run: &ConversationRunSpec,
) -> StorageResult<AppliedGraphDefinition> {
    run.validate()
        .map_err(|message| StorageError::InvalidArgument(message.into()))?;
    let revision = load_revision(connection, &run.graph_revision_id).await?;
    validate_conversation_run_contract(&revision.definition, run)
        .map_err(|message| StorageError::InvalidArgument(message.into()))?;
    Ok(revision.definition)
}
