use async_trait::async_trait;
use zhuangsheng_core::{
    application::ApplicationError,
    scheduler::{ClaimedAttempt, FinalizeAttemptCommand, SchedulerStore, SchedulerWork},
};

use crate::SqliteStore;

#[async_trait]
impl SchedulerStore for SqliteStore {
    async fn process_due_timers(&self, now_ms: i64) -> Result<u64, ApplicationError> {
        SqliteStore::process_due_timers(self, now_ms)
            .await
            .map_err(Into::into)
    }

    async fn recover_expired_leases(&self, now_ms: i64) -> Result<u64, ApplicationError> {
        SqliteStore::recover_expired_leases(self, now_ms)
            .await
            .map_err(Into::into)
    }

    async fn claim_next_work(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_until: i64,
    ) -> Result<Option<SchedulerWork>, ApplicationError> {
        SqliteStore::claim_next_work(self, worker_id, now_ms, lease_until)
            .await
            .map_err(Into::into)
    }

    async fn mark_attempt_running(
        &self,
        attempt: &ClaimedAttempt,
        now_ms: i64,
    ) -> Result<(), ApplicationError> {
        SqliteStore::mark_attempt_running(self, attempt, now_ms)
            .await
            .map_err(Into::into)
    }

    async fn finalize_attempt(
        &self,
        command: FinalizeAttemptCommand,
        now_ms: i64,
    ) -> Result<(), ApplicationError> {
        SqliteStore::finalize_attempt(self, command, now_ms)
            .await
            .map_err(Into::into)
    }

    async fn activate_if_ready(
        &self,
        wakeup_id: &str,
        run_id: &str,
        node_id: &str,
        now_ms: i64,
    ) -> Result<(), ApplicationError> {
        SqliteStore::activate_if_ready(self, wakeup_id, run_id, node_id, now_ms)
            .await
            .map_err(Into::into)
    }

    async fn settle_run(
        &self,
        wakeup_id: &str,
        run_id: &str,
        now_ms: i64,
    ) -> Result<(), ApplicationError> {
        SqliteStore::settle_run(self, wakeup_id, run_id, now_ms)
            .await
            .map_err(Into::into)
    }

    async fn checkpoint_run(&self, run_id: &str, now_ms: i64) -> Result<(), ApplicationError> {
        self.create_runtime_checkpoint(run_id, now_ms)
            .await
            .map(|_| ())
            .map_err(Into::into)
    }
}
