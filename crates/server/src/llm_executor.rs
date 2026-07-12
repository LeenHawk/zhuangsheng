use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        secret::{ResolveRuntimeSecretCommand, RuntimeSecretResolution, RuntimeSecretResolver},
    },
    llm::{
        ChannelCredential, InitialToolBatchInput, InitialToolBatchPlan, LlmRequestBuildInput,
        build_llm_request,
        context::{ContextAssemblyInput, ContextBudgetInput, ContextCountSource, assemble_context},
        plan_initial_tool_batch,
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution, LlmAttemptExecutor},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    StreamEventHub,
    llm_executor_support::*,
    provider::{HttpProviderClient, ProviderHttpError, ProviderTransport},
    tool_executor::ToolExecutorRegistry,
};

mod attempt_resume;
mod hosted_tools;
mod model_call;
mod model_completed_resume;
mod model_completion;
mod model_effect;
mod model_retry;
mod model_stream;
mod model_stream_batch;
mod model_stream_failure;
mod model_transport;
mod output_repair;
mod tool_dispatch;
mod tool_retry;

use attempt_resume::{AttemptResume, resume_attempt};
use model_call::{ModelCallInput, ModelCallResult, run_model_call};
#[cfg(test)]
pub(crate) use model_completed_resume::CompletedModelPause;
#[cfg(test)]
pub(crate) use output_repair::RepairPreparedPause;
use output_repair::{OutputDecision, finalize_or_prepare_repair};
use tool_dispatch::{ToolDispatchResult, dispatch_tool_batch};

pub struct LocalLlmExecutor {
    store: Arc<SqliteStore>,
    provider: Arc<dyn ProviderTransport>,
    tools: ToolExecutorRegistry,
    stream_events: StreamEventHub,
    #[cfg(test)]
    repair_pause: Option<Arc<RepairPreparedPause>>,
    #[cfg(test)]
    completed_model_pause: Option<Arc<CompletedModelPause>>,
}

impl LocalLlmExecutor {
    pub fn new(store: Arc<SqliteStore>) -> Result<Self, ProviderHttpError> {
        Ok(Self {
            store,
            provider: Arc::new(HttpProviderClient::new()?),
            tools: ToolExecutorRegistry::with_builtins(),
            stream_events: StreamEventHub::new(),
            #[cfg(test)]
            repair_pause: None,
            #[cfg(test)]
            completed_model_pause: None,
        })
    }

    pub fn with_provider(store: Arc<SqliteStore>, provider: Arc<dyn ProviderTransport>) -> Self {
        Self {
            store,
            provider,
            tools: ToolExecutorRegistry::with_builtins(),
            stream_events: StreamEventHub::new(),
            #[cfg(test)]
            repair_pause: None,
            #[cfg(test)]
            completed_model_pause: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_provider_and_tools(
        store: Arc<SqliteStore>,
        provider: Arc<dyn ProviderTransport>,
        tools: ToolExecutorRegistry,
    ) -> Self {
        Self {
            store,
            provider,
            tools,
            stream_events: StreamEventHub::new(),
            repair_pause: None,
            completed_model_pause: None,
        }
    }

    pub fn with_stream_events(mut self, stream_events: StreamEventHub) -> Self {
        self.stream_events = stream_events;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_repair_pause(mut self, pause: Arc<RepairPreparedPause>) -> Self {
        self.repair_pause = Some(pause);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_completed_model_pause(mut self, pause: Arc<CompletedModelPause>) -> Self {
        self.completed_model_pause = Some(pause);
        self
    }
}

#[async_trait]
impl LlmAttemptExecutor for LocalLlmExecutor {
    async fn execute_llm_attempt(
        &self,
        attempt: &ClaimedAttempt,
        now: i64,
    ) -> Result<LlmAttemptExecution, ApplicationError> {
        let Some(execution) = &attempt.execution_snapshot else {
            return Ok(finalize_failure(
                "llm_execution_snapshot_missing",
                "LLM execution snapshot is missing",
            ));
        };
        let Some(context_snapshot) = &attempt.context_snapshot else {
            return Ok(finalize_failure(
                "llm_context_snapshot_missing",
                "LLM context read snapshot is missing",
            ));
        };
        let node_input = Value::Object(
            attempt
                .inputs
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let (context_window, reserved_output) = match (
            execution.limits.max_input_tokens,
            execution.limits.max_output_tokens,
        ) {
            (Some(input), Some(output)) if input > output => (input, output),
            _ => {
                return Ok(finalize_failure(
                    "llm_token_limits_invalid",
                    "LLM token limits are missing or incompatible",
                ));
            }
        };
        let safety_margin = (context_window / 20)
            .max(256)
            .min(context_window.saturating_sub(reserved_output));
        let assembly = match assemble_context(
            &ContextAssemblyInput {
                node_input,
                config: execution.context.clone(),
                bindings: context_snapshot.bindings.clone(),
                budget: ContextBudgetInput {
                    context_window_tokens: context_window,
                    reserved_output_tokens: reserved_output,
                    fixed_request_tokens: fixed_request_estimate(execution),
                    safety_margin_tokens: safety_margin,
                    count_source: ContextCountSource::Estimate,
                },
                read_set_ref: context_snapshot.read_set_ref.clone(),
                read_set_digest: context_snapshot.read_set_digest.clone(),
                allow_sensitive: false,
            },
            &EstimateTokenCounter,
        ) {
            Ok(output) => output,
            Err(error) => return Ok(assembly_failure(error)),
        };
        let credential = match &execution.channel.spec.credential {
            ChannelCredential::Secret { api_key_ref } => {
                match RuntimeSecretResolver::resolve_runtime_secret(
                    self.store.as_ref(),
                    api_key_ref,
                    ResolveRuntimeSecretCommand {
                        run_id: attempt.run_id.clone(),
                        node_instance_id: attempt.node_instance_id.clone(),
                        attempt_id: attempt.attempt_id.clone(),
                        wakeup_id: attempt.wakeup_id.clone(),
                        worker_id: attempt.worker_id.clone(),
                        lease_fence: attempt.lease_fence,
                        run_control_epoch: attempt.run_control_epoch,
                        channel_id: execution.channel.channel_id.clone(),
                        read_set_digest: context_snapshot.read_set_digest.clone(),
                    },
                    now,
                )
                .await
                {
                    Ok(RuntimeSecretResolution::Resolved(value)) => Some(value),
                    Ok(RuntimeSecretResolution::Waiting { .. }) => {
                        return Ok(LlmAttemptExecution::Handled);
                    }
                    Err(_) => {
                        return Ok(finalize_failure(
                            "provider_credential_unavailable",
                            "provider credential is unavailable",
                        ));
                    }
                }
            }
            ChannelCredential::None => None,
        };
        let base_transcript_len = assembly.messages.len();
        let resume = resume_attempt(
            self,
            attempt,
            execution,
            &assembly,
            base_transcript_len,
            credential.as_ref(),
            reserved_output,
            now,
        )
        .await?;
        let resume = match resume {
            AttemptResume::Continue(resume) => resume,
            AttemptResume::Terminal(result) => return Ok(result),
        };
        let mut transcript_tail = resume.transcript_tail;
        let mut prior_checkpoint = resume.prior_checkpoint;
        let mut recovered_completed = resume.recovered_completed;
        let mut output_repairs_used = resume.output_repairs_used;
        loop {
            let completed = if let Some(completed) = recovered_completed.take() {
                completed
            } else {
                let call_no = prior_checkpoint.as_ref().map_or(
                    1,
                    |checkpoint: &zhuangsheng_core::llm::LlmLoopCheckpoint| {
                        checkpoint.model_calls_used.saturating_add(1)
                    },
                );
                let built = match build_llm_request(LlmRequestBuildInput {
                    execution,
                    context: &assembly,
                    registry_snapshot: &execution.tool_registry,
                    tool_descriptors: &execution.tool_descriptors,
                    transcript_tail: &transcript_tail,
                    continuation: None,
                    approved_hosted_bindings: &BTreeSet::new(),
                    model_call_no: call_no,
                }) {
                    Ok(output) => output,
                    Err(error) => return Ok(finalize_failure(error.code, &error.message)),
                };
                match run_model_call(
                    self,
                    ModelCallInput {
                        attempt,
                        execution,
                        built,
                        prior_checkpoint: prior_checkpoint.take(),
                        credential: credential.as_ref(),
                        reserved_output,
                        now,
                    },
                )
                .await?
                {
                    ModelCallResult::Completed(completed) => *completed,
                    ModelCallResult::Terminal(result) => return Ok(result),
                }
            };
            #[cfg(test)]
            if let Some(pause) = &self.completed_model_pause {
                pause.wait_once().await;
            }
            let tool_plan = match plan_initial_tool_batch(InitialToolBatchInput {
                execution,
                request_tools: &completed.resolved_tools,
                response_items: &completed.decoded.response.items,
                model_call_id: &completed.model_call_id,
                node_instance_id: &attempt.node_instance_id,
                originating_attempt_id: &attempt.attempt_id,
                checkpoint: completed.checkpoint.clone(),
                now_ms: now,
            }) {
                Ok(plan) => plan,
                Err(error) => return Ok(finalize_failure(error.code, &error.message)),
            };
            match tool_plan {
                InitialToolBatchPlan::Approval(command) => {
                    self.store
                        .prepare_tool_approval_batch(command, now)
                        .await
                        .map_err(ApplicationError::from)?;
                    return Ok(LlmAttemptExecution::Handled);
                }
                InitialToolBatchPlan::Executable(batch) => {
                    let settled =
                        match dispatch_tool_batch(self, attempt, execution, batch, now).await? {
                            ToolDispatchResult::Settled(settled) => *settled,
                            ToolDispatchResult::Terminal(result) => return Ok(result),
                        };
                    transcript_tail = settled
                        .transcript
                        .get(base_transcript_len..)
                        .ok_or(ApplicationError::Internal)?
                        .to_vec();
                    prior_checkpoint = Some(settled.checkpoint);
                }
                InitialToolBatchPlan::NoCalls => {
                    match finalize_or_prepare_repair(
                        self,
                        attempt,
                        execution,
                        completed,
                        output_repairs_used,
                        now,
                    )
                    .await?
                    {
                        OutputDecision::Final(result) => return Ok(result),
                        OutputDecision::Repair(continuation) => {
                            transcript_tail = continuation
                                .transcript
                                .get(base_transcript_len..)
                                .ok_or(ApplicationError::Internal)?
                                .to_vec();
                            prior_checkpoint = Some(continuation.checkpoint);
                            output_repairs_used = continuation.repairs_used;
                        }
                    }
                }
            }
        }
    }
}
