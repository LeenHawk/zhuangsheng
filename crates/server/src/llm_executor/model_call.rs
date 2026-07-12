use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    graph::LlmNodeExecutionSnapshot,
    llm::{
        ActiveModelEffectCheckpoint, CompletedResumeCountCall, LlmLogicalCallStatus,
        LlmLoopCheckpoint, LlmRequestBuildOutput, PrepareInitialModelCallCommand,
        PrepareModelCallCommand, ResolvedRequestTool, RetryReadyResumeCountCall,
        adapter::{AdapterExecutionOptions, DecodedTerminalDraft, encode_generation_request},
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::{finalize_failure, new_id};

use super::{
    LocalLlmExecutor,
    counting::count_request,
    counting_provider::{durable_count_request, provider_count_wire},
    counting_state::{CountRequestInput, count_candidate_bytes},
    model_effect::{model_effect, model_retry_policy},
    model_request::durable_generation_request,
    model_transport::{PreparedModelCallInput, execute_prepared_model_call},
    opaque_resources::resolve_opaque_resources,
};

pub(super) enum ModelCallResult {
    Completed(Box<CompletedModelCall>),
    Reassemble {
        checkpoint: Box<LlmLoopCheckpoint>,
        token_count: u64,
        overage: u64,
        source: zhuangsheng_core::llm::CountResultSource,
    },
    Terminal(LlmAttemptExecution),
}

pub(super) struct ModelCallInput<'a> {
    pub attempt: &'a ClaimedAttempt,
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub built: LlmRequestBuildOutput,
    pub prior_checkpoint: Option<LlmLoopCheckpoint>,
    pub retry_count: Option<RetryReadyResumeCountCall>,
    pub completed_count: Option<CompletedResumeCountCall>,
    pub input_token_limit: u64,
    pub credential: Option<&'a SecretValue>,
    pub reserved_output: u64,
    pub now: i64,
}

pub(super) struct CompletedModelCall {
    pub model_call_id: String,
    pub checkpoint: LlmLoopCheckpoint,
    pub decoded: DecodedTerminalDraft,
    pub resolved_tools: Vec<ResolvedRequestTool>,
    pub resolved_memory_tools: Vec<zhuangsheng_core::llm::ResolvedMemoryTool>,
    pub transcript: Vec<zhuangsheng_core::llm::ir::LlmTurnItemIr>,
}

pub(super) async fn run_model_call(
    executor: &LocalLlmExecutor,
    input: ModelCallInput<'_>,
) -> Result<ModelCallResult, ApplicationError> {
    let ModelCallInput {
        attempt,
        execution,
        built,
        prior_checkpoint,
        retry_count,
        completed_count,
        input_token_limit,
        credential,
        reserved_output,
        now,
    } = input;
    let resources =
        resolve_opaque_resources(executor, &execution.operation, &built.request, now).await?;
    let options = AdapterExecutionOptions {
        stream: execution
            .streaming
            .as_ref()
            .is_some_and(|streaming| streaming.enabled),
        max_output_tokens: reserved_output,
    };
    let wire = match encode_generation_request(
        &execution.operation,
        &built.request,
        &resources,
        options,
    ) {
        Ok(wire) => wire,
        Err(error) => {
            return Ok(ModelCallResult::Terminal(finalize_failure(
                error.code,
                &error.message,
            )));
        }
    };
    let durable_request =
        durable_generation_request(&execution.operation, &built.request, options)?;
    let provider_count_wire = provider_count_wire(execution, &wire);
    let count_request_bytes = durable_count_request(&built.request, provider_count_wire.as_ref())?;
    let counted = count_request(
        executor,
        CountRequestInput {
            attempt,
            execution,
            transcript: &built.request.transcript,
            candidate_bytes: count_candidate_bytes(&built.request)?,
            request_bytes: count_request_bytes,
            prior_checkpoint,
            retry: retry_count,
            completed: completed_count,
            provider_wire: provider_count_wire,
            credential,
            now,
        },
    )
    .await?;
    if let Some(result) = counted.result.as_ref()
        && result.token_count > input_token_limit
    {
        let count_limit = execution
            .limits
            .max_count_calls
            .ok_or(ApplicationError::Internal)?;
        if counted.checkpoint.count_calls_used >= count_limit {
            return Ok(ModelCallResult::Terminal(finalize_failure(
                "llm_count_budget_exceeded",
                "provider token count exceeds the input budget after bounded trimming",
            )));
        }
        return Ok(ModelCallResult::Reassemble {
            checkpoint: Box::new(counted.checkpoint),
            token_count: result.token_count,
            overage: result.token_count - input_token_limit,
            source: result.source,
        });
    }
    let prior_checkpoint = Some(counted.checkpoint);
    let model_call_id = new_id("modelcall");
    let effect_id = new_id("effect");
    let effect_attempt_id = new_id("effectattempt");
    let effect = model_effect(&execution.hosted_tools);
    let checkpoint = if let Some(mut checkpoint) = prior_checkpoint {
        let call_no = checkpoint.model_calls_used.saturating_add(1);
        checkpoint.model_call_no = call_no;
        checkpoint.model_calls_used = call_no;
        checkpoint.last_updated_by_attempt_id = attempt.attempt_id.clone();
        checkpoint.current_batch.clear();
        checkpoint.active_model_effect = Some(ActiveModelEffectCheckpoint {
            model_call_id: model_call_id.clone(),
            effect_id: effect_id.clone(),
            status: LlmLogicalCallStatus::Prepared,
            response_ref: None,
        });
        checkpoint.effect_watermark = effect_attempt_id.clone();
        checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
        executor
            .store
            .prepare_model_call(
                PrepareModelCallCommand {
                    model_call_id: model_call_id.clone(),
                    effect_id: effect_id.clone(),
                    effect_attempt_id: effect_attempt_id.clone(),
                    node_instance_id: attempt.node_instance_id.clone(),
                    originating_attempt_id: attempt.attempt_id.clone(),
                    fence: model_fence(attempt),
                    call_no,
                    channel_id: execution.channel.channel_id.clone(),
                    operation: execution.operation.clone(),
                    request_bytes: durable_request,
                    effect_kind: effect.kind.into(),
                    effect_classification: effect.classification,
                    effect_operation_key: effect.operation_key.into(),
                    effect_idempotency_key: model_call_id.clone(),
                    retry_policy: model_retry_policy(effect.classification),
                    checkpoint: checkpoint.clone(),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?;
        checkpoint
    } else {
        executor
            .store
            .prepare_initial_model_call(
                PrepareInitialModelCallCommand {
                    model_call_id: model_call_id.clone(),
                    effect_id: effect_id.clone(),
                    effect_attempt_id: effect_attempt_id.clone(),
                    node_instance_id: attempt.node_instance_id.clone(),
                    originating_attempt_id: attempt.attempt_id.clone(),
                    fence: model_fence(attempt),
                    channel_id: execution.channel.channel_id.clone(),
                    operation: execution.operation.clone(),
                    request_bytes: durable_request,
                    transcript: built.request.transcript.clone(),
                    registry_snapshot: execution.tool_registry.clone(),
                    read_set_digest: attempt
                        .context_snapshot
                        .as_ref()
                        .ok_or(ApplicationError::Internal)?
                        .read_set_digest
                        .clone(),
                    effect_kind: effect.kind.into(),
                    effect_classification: effect.classification,
                    effect_operation_key: effect.operation_key.into(),
                    effect_idempotency_key: model_call_id.clone(),
                    retry_policy: model_retry_policy(effect.classification),
                },
                now,
            )
            .await
            .map_err(ApplicationError::from)?
            .checkpoint
    };
    execute_prepared_model_call(
        executor,
        PreparedModelCallInput {
            attempt,
            execution,
            built,
            wire,
            model_call_id,
            effect_attempt_id,
            checkpoint,
            credential,
            now,
        },
    )
    .await
}

fn model_fence(attempt: &ClaimedAttempt) -> zhuangsheng_core::llm::EffectAttemptFence {
    zhuangsheng_core::llm::EffectAttemptFence {
        invoking_node_attempt_id: attempt.attempt_id.clone(),
        worker_id: attempt.worker_id.clone(),
        lease_fence: attempt.lease_fence,
        run_control_epoch: attempt.run_control_epoch,
    }
}
