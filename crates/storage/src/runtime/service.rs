use async_trait::async_trait;
use zhuangsheng_core::{
    application::ApplicationError,
    runtime::{
        ContextBranchView, DurableRunEventView, ForkContextCommand, RunControlCommand,
        RunOutputsView, RunView, RuntimeService, StartRunCommand, SubmitWaitResponseCommand,
        WaitDeliveryView, WaitView,
    },
};

use crate::{SqliteStore, graph::helpers::now_ms};

#[async_trait]
impl RuntimeService for SqliteStore {
    async fn start_run(&self, command: StartRunCommand) -> Result<RunView, ApplicationError> {
        SqliteStore::start_run(self, command)
            .await
            .map_err(Into::into)
    }

    async fn get_run(&self, run_id: &str) -> Result<RunView, ApplicationError> {
        SqliteStore::get_run(self, run_id).await.map_err(Into::into)
    }

    async fn get_run_outputs(&self, run_id: &str) -> Result<RunOutputsView, ApplicationError> {
        SqliteStore::get_run_outputs(self, run_id)
            .await
            .map_err(Into::into)
    }

    async fn list_open_waits(&self, run_id: &str) -> Result<Vec<WaitView>, ApplicationError> {
        SqliteStore::list_open_waits(self, run_id)
            .await
            .map_err(Into::into)
    }

    async fn list_run_events(
        &self,
        run_id: &str,
        after_durable_seq: u64,
        limit: u32,
    ) -> Result<Vec<DurableRunEventView>, ApplicationError> {
        SqliteStore::list_run_events(self, run_id, after_durable_seq, limit)
            .await
            .map_err(Into::into)
    }

    async fn load_json_value_bytes(&self, value_ref: &str) -> Result<Vec<u8>, ApplicationError> {
        SqliteStore::load_json_value_bytes(self, value_ref)
            .await
            .map_err(Into::into)
    }

    async fn fork_context(
        &self,
        command: ForkContextCommand,
    ) -> Result<ContextBranchView, ApplicationError> {
        self.fork_context_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn request_interrupt(
        &self,
        command: RunControlCommand,
    ) -> Result<RunView, ApplicationError> {
        SqliteStore::request_interrupt(self, command)
            .await
            .map_err(Into::into)
    }

    async fn resume_interrupted(
        &self,
        command: RunControlCommand,
    ) -> Result<RunView, ApplicationError> {
        SqliteStore::resume_interrupted(self, command)
            .await
            .map_err(Into::into)
    }

    async fn request_cancel(
        &self,
        command: RunControlCommand,
    ) -> Result<RunView, ApplicationError> {
        SqliteStore::request_cancel(self, command)
            .await
            .map_err(Into::into)
    }

    async fn submit_wait_response(
        &self,
        command: SubmitWaitResponseCommand,
    ) -> Result<WaitDeliveryView, ApplicationError> {
        SqliteStore::submit_wait_response(self, command, now_ms())
            .await
            .map_err(Into::into)
    }
}
