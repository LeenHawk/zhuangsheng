use zhuangsheng_core::{
    application::{ApplicationError, secret::SecretValue},
    graph::LlmNodeExecutionSnapshot,
    llm::{
        ActiveModelEffectCheckpoint, LlmLogicalCallStatus, LlmLoopCheckpoint,
        LlmRequestBuildOutput, PrepareInitialModelCallCommand, PrepareModelCallCommand,
        ResolvedRequestTool,
        adapter::{
            AdapterExecutionOptions, AdapterResources, DecodedTerminalDraft,
            encode_generation_request,
        },
    },
    scheduler::{ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::{finalize_failure, new_id};

use super::{
    LocalLlmExecutor,
    model_effect::{model_effect, model_retry_policy},
    model_transport::{PreparedModelCallInput, execute_prepared_model_call},
};

pub(super) enum ModelCallResult {
    Completed(Box<CompletedModelCall>),
    Terminal(LlmAttemptExecution),
}

pub(super) struct ModelCallInput<'a> {
    pub attempt: &'a ClaimedAttempt,
    pub execution: &'a LlmNodeExecutionSnapshot,
    pub built: LlmRequestBuildOutput,
    pub prior_checkpoint: Option<LlmLoopCheckpoint>,
    pub credential: Option<&'a SecretValue>,
    pub reserved_output: u64,
    pub now: i64,
}

pub(super) struct CompletedModelCall {
    pub model_call_id: String,
    pub checkpoint: LlmLoopCheckpoint,
    pub decoded: DecodedTerminalDraft,
    pub resolved_tools: Vec<ResolvedRequestTool>,
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
        credential,
        reserved_output,
        now,
    } = input;
    let wire = match encode_generation_request(
        &execution.operation,
        &built.request,
        &AdapterResources::default(),
        AdapterExecutionOptions {
            stream: execution
                .streaming
                .as_ref()
                .is_some_and(|streaming| streaming.enabled),
            max_output_tokens: reserved_output,
        },
    ) {
        Ok(wire) => wire,
        Err(error) => {
            return Ok(ModelCallResult::Terminal(finalize_failure(
                error.code,
                &error.message,
            )));
        }
    };
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
                    request_bytes: wire.body().to_vec(),
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
                    request_bytes: wire.body().to_vec(),
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
