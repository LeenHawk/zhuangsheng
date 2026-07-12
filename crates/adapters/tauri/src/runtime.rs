use zhuangsheng_core::{
    context_merge::{MergeContextCommand, MergeContextView},
    llm::{EffectResolutionView, ResolveEffectUnknownCommand},
    runtime::{
        ContextBranchView, DurableRunEventView, ForkContextCommand, RunControlCommand, RunListView,
        RunView, StartRunCommand, SubmitWaitResponseCommand, WaitDeliveryView, WaitView,
    },
};

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn start_run(&self, command: StartRunCommand) -> CommandResult<RunView> {
        Ok(self.runtime.start_run(command).await?)
    }

    pub async fn get_run(&self, run_id: &str) -> CommandResult<RunView> {
        Ok(self.runtime.get_run(run_id).await?)
    }

    pub async fn list_recent_runs(&self, limit: u32) -> CommandResult<RunListView> {
        Ok(self.runtime.list_recent_runs(limit.min(100)).await?)
    }

    pub async fn list_open_waits(&self, run_id: &str) -> CommandResult<Vec<WaitView>> {
        Ok(self.runtime.list_open_waits(run_id).await?)
    }

    pub async fn list_run_events(
        &self,
        run_id: &str,
        after: u64,
        limit: u32,
    ) -> CommandResult<Vec<DurableRunEventView>> {
        Ok(self
            .runtime
            .list_run_events(run_id, after, limit.min(500))
            .await?)
    }

    pub async fn interrupt_run(&self, command: RunControlCommand) -> CommandResult<RunView> {
        Ok(self.runtime.request_interrupt(command).await?)
    }

    pub async fn resume_run(&self, command: RunControlCommand) -> CommandResult<RunView> {
        Ok(self.runtime.resume_interrupted(command).await?)
    }

    pub async fn cancel_run(&self, command: RunControlCommand) -> CommandResult<RunView> {
        Ok(self.runtime.request_cancel(command).await?)
    }

    pub async fn satisfy_wait(
        &self,
        command: SubmitWaitResponseCommand,
    ) -> CommandResult<WaitDeliveryView> {
        Ok(self.runtime.submit_wait_response(command).await?)
    }

    pub async fn resolve_effect_unknown(
        &self,
        command: ResolveEffectUnknownCommand,
    ) -> CommandResult<EffectResolutionView> {
        Ok(self.runtime.resolve_effect_unknown(command).await?)
    }

    pub async fn fork_context(
        &self,
        command: ForkContextCommand,
    ) -> CommandResult<ContextBranchView> {
        Ok(self.runtime.fork_context(command).await?)
    }

    pub async fn merge_context(
        &self,
        command: MergeContextCommand,
    ) -> CommandResult<MergeContextView> {
        Ok(self.runtime.merge_context(command).await?)
    }
}
