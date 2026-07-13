use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::memory::MemoryChangeProposalView;

use crate::{
    StorageResult,
    runtime::{Event, append_event},
};

pub(super) async fn append_proposal_run_event<C: ConnectionTrait>(
    connection: &C,
    proposal: &MemoryChangeProposalView,
    event_type: &str,
    status: &str,
    now: i64,
) -> StorageResult<()> {
    let Some(run_id) = &proposal.origin_run_id else {
        return Ok(());
    };
    append_event(
        connection,
        Event {
            run_id,
            event_type,
            importance: "critical",
            node_instance_id: proposal.origin_node_instance_id.as_deref(),
            attempt_id: None,
            payload: json!({
                "schemaVersion":1,
                "proposalId":proposal.id,
                "memoryId":proposal.memory_id,
                "scopeId":proposal.scope_id,
                "status":status,
                "appliedCommitId":proposal.applied_commit_id,
            }),
            now,
        },
    )
    .await?;
    Ok(())
}
