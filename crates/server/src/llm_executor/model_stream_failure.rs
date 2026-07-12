use zhuangsheng_core::{
    application::ApplicationError,
    canonical,
    llm::{
        EffectAttemptFence, FinishModelCallCommand, LlmLogicalCallStatus, LlmLoopCheckpoint,
        ModelCallEffectOutcome,
    },
};

use crate::llm_executor_support::{finalize_failure, set_model_status};

use super::{LocalLlmExecutor, model_call::ModelCallResult};

pub(super) async fn fail_protocol(
    executor: &LocalLlmExecutor,
    effect_attempt_id: String,
    fence: EffectAttemptFence,
    checkpoint: LlmLoopCheckpoint,
    code: &str,
    message: &str,
    now: i64,
) -> Result<ModelCallResult, ApplicationError> {
    let bytes = canonical::to_vec(&serde_json::json!({"code":code,"message":message}))
        .map_err(|_| ApplicationError::Internal)?;
    fail_stream(
        executor,
        effect_attempt_id,
        fence,
        checkpoint,
        code,
        message,
        bytes,
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn fail_stream(
    executor: &LocalLlmExecutor,
    effect_attempt_id: String,
    fence: EffectAttemptFence,
    mut checkpoint: LlmLoopCheckpoint,
    code: &str,
    message: &str,
    error_bytes: Vec<u8>,
    now: i64,
) -> Result<ModelCallResult, ApplicationError> {
    set_model_status(&mut checkpoint, LlmLogicalCallStatus::Failed);
    checkpoint = checkpoint.seal().map_err(|_| ApplicationError::Internal)?;
    executor
        .store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id,
                fence,
                outcome: ModelCallEffectOutcome::Failed { error_bytes },
                checkpoint,
                transcript: None,
            },
            now,
        )
        .await
        .map_err(ApplicationError::from)?;
    Ok(ModelCallResult::Terminal(finalize_failure(code, message)))
}
