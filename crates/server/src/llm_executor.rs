use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        secret::{ResolveRuntimeSecretCommand, RuntimeSecretResolution, RuntimeSecretResolver},
    },
    graph::EffectClassification,
    llm::{
        ChannelCredential, EffectAttemptFence, EffectRetryPolicy, FinishModelCallCommand,
        InitialToolBatchInput, InitialToolBatchPlan, LlmLogicalCallStatus, LlmRequestBuildInput,
        ModelCallEffectOutcome, PrepareInitialModelCallCommand, StartModelCallCommand,
        ToolRegistrySnapshot,
        adapter::{
            AdapterExecutionOptions, AdapterResources, decode_generation_terminal,
            encode_generation_request,
        },
        build_llm_request,
        context::{ContextAssemblyInput, ContextBudgetInput, ContextCountSource, assemble_context},
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution, LlmAttemptExecutor},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor_support::*,
    provider::{HttpProviderClient, ProviderHttpError, ProviderTransport},
};

pub struct LocalLlmExecutor {
    store: Arc<SqliteStore>,
    provider: Arc<dyn ProviderTransport>,
}

impl LocalLlmExecutor {
    pub fn new(store: Arc<SqliteStore>) -> Result<Self, ProviderHttpError> {
        Ok(Self {
            store,
            provider: Arc::new(HttpProviderClient::new()?),
        })
    }

    pub fn with_provider(store: Arc<SqliteStore>, provider: Arc<dyn ProviderTransport>) -> Self {
        Self { store, provider }
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
        if execution
            .streaming
            .as_ref()
            .is_some_and(|value| value.enabled)
        {
            return Ok(finalize_failure(
                "llm_streaming_executor_pending",
                "streaming LLM execution is not connected yet",
            ));
        }
        if !execution.hosted_tools.is_empty() {
            return Ok(finalize_failure(
                "llm_hosted_tool_executor_pending",
                "hosted tool execution is not connected yet",
            ));
        }
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
        let registry: ToolRegistrySnapshot = execution.tool_registry.clone();
        let built = match build_llm_request(LlmRequestBuildInput {
            execution,
            context: &assembly,
            registry_snapshot: &registry,
            tool_descriptors: &execution.tool_descriptors,
            transcript_tail: &[],
            continuation: None,
            approved_hosted_bindings: &BTreeSet::new(),
            model_call_no: 1,
        }) {
            Ok(output) => output,
            Err(error) => return Ok(finalize_failure(error.code, &error.message)),
        };
        let wire = match encode_generation_request(
            &execution.operation,
            &built.request,
            &AdapterResources::default(),
            AdapterExecutionOptions {
                stream: false,
                max_output_tokens: reserved_output,
            },
        ) {
            Ok(wire) => wire,
            Err(error) => return Ok(finalize_failure(error.code, &error.message)),
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
        let model_call_id = new_id("modelcall");
        let effect_id = new_id("effect");
        let effect_attempt_id = new_id("effectattempt");
        let prepared = self
            .store
            .prepare_initial_model_call(
                PrepareInitialModelCallCommand {
                    model_call_id: model_call_id.clone(),
                    effect_id: effect_id.clone(),
                    effect_attempt_id: effect_attempt_id.clone(),
                    node_instance_id: attempt.node_instance_id.clone(),
                    originating_attempt_id: attempt.attempt_id.clone(),
                    channel_id: execution.channel.channel_id.clone(),
                    operation: execution.operation.clone(),
                    request_bytes: wire.body().to_vec(),
                    transcript: built.request.transcript.clone(),
                    registry_snapshot: registry,
                    read_set_digest: context_snapshot.read_set_digest.clone(),
                    effect_kind: "model_generation".into(),
                    effect_classification: EffectClassification::Idempotent,
                    effect_operation_key: "llm.generate".into(),
                    effect_idempotency_key: model_call_id.clone(),
                    retry_policy: EffectRetryPolicy {
                        max_attempts: 2,
                        backoff_ms: vec![250],
                    },
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        let fence = EffectAttemptFence {
            invoking_node_attempt_id: attempt.attempt_id.clone(),
            worker_id: attempt.worker_id.clone(),
            lease_fence: attempt.lease_fence,
            run_control_epoch: attempt.run_control_epoch,
        };
        let mut running = prepared.checkpoint.clone();
        set_model_status(&mut running, LlmLogicalCallStatus::Running);
        running = running.seal().map_err(|_| ApplicationError::Internal)?;
        self.store
            .start_model_call(
                StartModelCallCommand {
                    effect_attempt_id: effect_attempt_id.clone(),
                    fence: fence.clone(),
                    provider_request_id: None,
                    checkpoint: running.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        let response = self
            .provider
            .send(&execution.channel, &wire, credential.as_ref())
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let status = if error.outcome_unknown {
                    LlmLogicalCallStatus::OutcomeUnknown
                } else {
                    LlmLogicalCallStatus::Failed
                };
                let mut terminal = running;
                set_model_status(&mut terminal, status);
                terminal = terminal.seal().map_err(|_| ApplicationError::Internal)?;
                let error_bytes = provider_error_bytes(&error);
                self.store
                    .finish_model_call(
                        FinishModelCallCommand {
                            effect_attempt_id,
                            fence,
                            outcome: if error.outcome_unknown {
                                ModelCallEffectOutcome::OutcomeUnknown { error_bytes }
                            } else {
                                ModelCallEffectOutcome::Failed { error_bytes }
                            },
                            checkpoint: terminal,
                            transcript: None,
                        },
                        now,
                    )
                    .await
                    .map_err(ApplicationError::from)?;
                return Ok(if error.outcome_unknown {
                    LlmAttemptExecution::Handled
                } else {
                    finalize_failure(error.code, &error.safe_message)
                });
            }
        };
        let decoded =
            decode_generation_terminal(&execution.operation, &model_call_id, &response.body);
        let mut completed = running;
        set_model_status(&mut completed, LlmLogicalCallStatus::Completed);
        completed = completed.seal().map_err(|_| ApplicationError::Internal)?;
        let usage = decoded
            .as_ref()
            .ok()
            .and_then(|draft| draft.response.usage.clone());
        let durable_transcript = decoded
            .as_ref()
            .ok()
            .filter(|draft| {
                draft.sensitive_entries.is_empty() && draft.opaque_attachments.is_empty()
            })
            .map(|draft| {
                let mut transcript = built.request.transcript.clone();
                transcript.extend(draft.response.items.clone());
                transcript
            });
        let completed_checkpoint = self
            .store
            .finish_model_call(
                FinishModelCallCommand {
                    effect_attempt_id,
                    fence,
                    outcome: ModelCallEffectOutcome::Completed {
                        response_bytes: response.body,
                        usage,
                    },
                    checkpoint: completed,
                    transcript: durable_transcript,
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        let decoded = match decoded {
            Ok(value) => value,
            Err(error) => return Ok(finalize_failure(error.code, &error.message)),
        };
        if !decoded.sensitive_entries.is_empty() || !decoded.opaque_attachments.is_empty() {
            return Ok(finalize_failure(
                "llm_opaque_storage_pending",
                "provider response requires opaque continuation storage",
            ));
        }
        let tool_plan =
            match zhuangsheng_core::llm::plan_initial_tool_batch(InitialToolBatchInput {
                execution,
                request_tools: &built.resolved_tools,
                response_items: &decoded.response.items,
                model_call_id: &model_call_id,
                node_instance_id: &attempt.node_instance_id,
                originating_attempt_id: &attempt.attempt_id,
                checkpoint: completed_checkpoint,
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
            InitialToolBatchPlan::ExecutablePending => {
                return Ok(finalize_failure(
                    "llm_tool_dispatcher_pending",
                    "custom tool execution dispatcher is not connected yet",
                ));
            }
            InitialToolBatchPlan::NoCalls => {}
        }
        Ok(finalize_output(
            execution.output.as_ref(),
            &decoded.response.items,
        ))
    }
}
