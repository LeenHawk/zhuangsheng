use serde::{Deserialize, Serialize};
use ulid::Ulid;
use zhuangsheng_core::{
    context_merge::{MergeContextCommand, MergeContextView},
    llm::{
        EffectResolutionActorKind, EffectResolutionKind, EffectResolutionView,
        ResolveEffectUnknownCommand,
    },
    runtime::{
        ContextBranchView, DurableRunEventView, ForkContextCommand, RunControlCommand, RunListView,
        RunView, StartRunCommand, SubmitWaitResponseCommand, WaitDeliveryView, WaitResponsePayload,
        WaitView,
    },
};

use crate::{CommandResult, TauriAdapter};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SatisfyWaitInput {
    pub wait_id: String,
    pub delivery_id: String,
    pub response: WaitResponsePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveEffectUnknownInput {
    pub effect_id: String,
    pub expected_effect_attempt_id: String,
    pub expected_run_control_epoch: u64,
    pub kind: EffectResolutionKind,
    pub decision: serde_json::Value,
    pub result_object_id: Option<String>,
    pub evidence_object_id: Option<String>,
    pub idempotency_key: String,
}

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

    pub async fn satisfy_wait(&self, input: SatisfyWaitInput) -> CommandResult<WaitDeliveryView> {
        Ok(self
            .runtime
            .submit_wait_response(SubmitWaitResponseCommand {
                wait_id: input.wait_id,
                delivery_id: input.delivery_id,
                actor_kind: "human".into(),
                actor_id: Some("local-user".into()),
                payload: input.response,
            })
            .await?)
    }

    pub async fn resolve_effect_unknown(
        &self,
        input: ResolveEffectUnknownInput,
    ) -> CommandResult<EffectResolutionView> {
        Ok(self
            .runtime
            .resolve_effect_unknown(ResolveEffectUnknownCommand {
                resolution_id: format!("effectresolution_{}", Ulid::new()),
                effect_id: input.effect_id,
                expected_effect_attempt_id: input.expected_effect_attempt_id,
                expected_run_control_epoch: input.expected_run_control_epoch,
                command_idempotency_key: input.idempotency_key,
                kind: input.kind,
                decision: input.decision,
                result_object_id: input.result_object_id,
                evidence_object_id: input.evidence_object_id,
                actor_kind: EffectResolutionActorKind::Human,
                actor_id: Some("local-user".into()),
            })
            .await?)
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
