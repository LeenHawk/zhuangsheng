use zhuangsheng_core::{
    application::ApplicationError,
    graph::{LlmNodeExecutionSnapshot, LlmOutputSpec},
    llm::{
        EffectAttemptFence, PrepareLlmOutputRepairCommand, build_llm_output_repair_material,
        finalize_llm_output,
        ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole},
    },
    scheduler::{BuiltinResult, ClaimedAttempt, LlmAttemptExecution},
};

use crate::llm_executor_support::{finalize_failure, new_id};

use super::text_transform::apply_canonical_output_transforms;
use super::{LocalLlmExecutor, model_call::CompletedModelCall};

#[cfg(test)]
pub(crate) struct RepairPreparedPause {
    pub started: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}

#[cfg(test)]
impl RepairPreparedPause {
    pub fn new() -> Self {
        Self {
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        }
    }

    async fn wait(&self) {
        self.started.notify_one();
        self.release.notified().await;
    }
}

pub(super) enum OutputDecision {
    Final(LlmAttemptExecution),
    Repair(Box<OutputRepairContinuation>),
}

pub(super) struct OutputRepairContinuation {
    pub checkpoint: zhuangsheng_core::llm::LlmLoopCheckpoint,
    pub transcript: Vec<LlmTurnItemIr>,
    pub repairs_used: u64,
}

pub(super) async fn finalize_or_prepare_repair(
    executor: &LocalLlmExecutor,
    attempt: &ClaimedAttempt,
    execution: &LlmNodeExecutionSnapshot,
    completed: CompletedModelCall,
    repairs_used: u64,
    now: i64,
) -> Result<OutputDecision, ApplicationError> {
    match finalize_llm_output(
        execution.output.as_ref(),
        &completed.decoded.response.items,
        &completed.transcript,
    ) {
        Ok(value) => match apply_canonical_output_transforms(value, execution) {
            Ok(value) => Ok(OutputDecision::Final(LlmAttemptExecution::Finalize(
                BuiltinResult::Completed {
                    outputs: [("default".into(), value)].into_iter().collect(),
                },
            ))),
            Err(error) => Ok(OutputDecision::Final(finalize_failure(
                error.code,
                &error.message,
            ))),
        },
        Err(error) => {
            let Some(LlmOutputSpec::Json { .. }) = execution.output.as_ref() else {
                return Ok(OutputDecision::Final(finalize_failure(
                    error.code,
                    &error.message,
                )));
            };
            let repair_limit = execution
                .limits
                .max_output_repairs
                .ok_or(ApplicationError::Internal)?;
            let model_limit = execution
                .limits
                .max_model_calls
                .ok_or(ApplicationError::Internal)?;
            if repairs_used >= repair_limit || completed.checkpoint.model_calls_used >= model_limit
            {
                return Ok(OutputDecision::Final(finalize_failure(
                    error.code,
                    &error.message,
                )));
            }
            let material =
                build_llm_output_repair_material(&error, &completed.decoded.response.items);
            let repair_id = new_id("outputrepair");
            let prepared = executor
                .store
                .prepare_llm_output_repair(
                    PrepareLlmOutputRepairCommand {
                        repair_id: repair_id.clone(),
                        node_instance_id: attempt.node_instance_id.clone(),
                        source_model_call_id: completed.model_call_id,
                        extracted_bytes_digest: material.extracted_bytes_digest,
                        error_code: material.error_code,
                        instruction: LlmTurnItemIr::Message {
                            id: format!("{repair_id}:instruction"),
                            role: MessageRole::User,
                            content: vec![LlmContentPartIr::Text {
                                text: material.instruction,
                            }],
                            provenance: None,
                            placeholder: false,
                        },
                        fence: EffectAttemptFence {
                            invoking_node_attempt_id: attempt.attempt_id.clone(),
                            worker_id: attempt.worker_id.clone(),
                            lease_fence: attempt.lease_fence,
                            run_control_epoch: attempt.run_control_epoch,
                        },
                        checkpoint: completed.checkpoint,
                    },
                    now,
                )
                .await
                .map_err(ApplicationError::from)?;
            #[cfg(test)]
            if let Some(pause) = &executor.repair_pause {
                pause.wait().await;
            }
            Ok(OutputDecision::Repair(Box::new(OutputRepairContinuation {
                checkpoint: prepared.checkpoint,
                transcript: prepared.transcript,
                repairs_used: prepared.repair_no,
            })))
        }
    }
}
