mod append;
mod complete;
mod jobs;
mod outcome;
mod payload;

pub(super) use append::append_ready_candidate;
pub(super) use complete::Candidate;
pub(super) use payload::{ReplyPayloadError, load_reply_payload};

use crate::{SqliteStore, StorageResult};

impl SqliteStore {
    pub async fn maintain_candidate_projections(
        &self,
        now: i64,
        worker_id: &str,
        limit: u32,
    ) -> StorageResult<u64> {
        jobs::maintain(self, now, worker_id, limit.clamp(1, 100)).await
    }
}
