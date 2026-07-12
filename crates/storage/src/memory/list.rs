use sea_orm::ConnectionTrait;
use zhuangsheng_core::application::memory::{
    ListMemoryProposalsCommand, MemoryProposalCursor, MemoryProposalListView,
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::query::{proposal_from_row, proposal_status};

const SELECT: &str = "SELECT id, scope_id, memory_id, expected_head_commit_id, change_type, content_object_id, reason, evidence_refs_json, requested_by_kind, requested_by_id, schema_version, policy_version, origin_run_id, origin_node_instance_id, applied_commit_id, status, created_at, updated_at FROM memory_change_proposals WHERE scope_id = ?";

impl SqliteStore {
    pub async fn list_memory_proposals(
        &self,
        command: ListMemoryProposalsCommand,
    ) -> StorageResult<MemoryProposalListView> {
        if command.scope_id.is_empty() || command.limit == 0 || command.limit > 100 {
            return Err(StorageError::InvalidArgument(
                "memory proposal list parameters are invalid".into(),
            ));
        }
        let mut query = SELECT.to_owned();
        let mut values = vec![command.scope_id.into()];
        if let Some(status) = command.status {
            query.push_str(" AND status = ?");
            values.push(proposal_status(status).into());
        }
        if let Some(cursor) = &command.cursor {
            if cursor.id.is_empty() {
                return Err(StorageError::InvalidArgument(
                    "memory proposal cursor is invalid".into(),
                ));
            }
            query.push_str(" AND (updated_at < ? OR (updated_at = ? AND id < ?))");
            values.push(cursor.updated_at.into());
            values.push(cursor.updated_at.into());
            values.push(cursor.id.clone().into());
        }
        query.push_str(" ORDER BY updated_at DESC, id DESC LIMIT ?");
        values.push(i64::from(command.limit + 1).into());
        let rows = self.db.query_all_raw(sql(&query, values)).await?;
        let has_more = rows.len() > command.limit as usize;
        let mut proposals = Vec::with_capacity(rows.len().min(command.limit as usize));
        for row in rows.iter().take(command.limit as usize) {
            proposals.push(proposal_from_row(&self.db, row).await?);
        }
        let next_cursor = has_more.then(|| {
            let last = proposals.last().expect("page with more rows is non-empty");
            MemoryProposalCursor {
                updated_at: last.updated_at,
                id: last.id.clone(),
            }
        });
        Ok(MemoryProposalListView {
            proposals,
            next_cursor,
        })
    }
}
