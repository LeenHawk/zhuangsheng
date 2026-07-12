use serde_json::{Value, json};
use ulid::Ulid;
use zhuangsheng_core::{
    application::ApplicationError,
    canonical,
    graph::{LlmNodeExecutionSnapshot, LlmOutputSpec},
    llm::{
        ActiveModelEffectCheckpoint, EffectAttemptFence, FinishModelCallCommand,
        LlmLogicalCallStatus, ModelCallEffectOutcome,
        context::{ContextAssemblyError, ContextAssemblyResult, ContextRole, ContextTokenCounter},
        ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole},
    },
    scheduler::{BuiltinResult, LlmAttemptExecution},
};

use crate::provider::ProviderHttpError;

pub(super) struct EstimateTokenCounter;

impl ContextTokenCounter for EstimateTokenCounter {
    fn count(
        &self,
        _role: ContextRole,
        content: &[LlmContentPartIr],
    ) -> ContextAssemblyResult<u64> {
        let mut tokens = 4u64;
        for part in content {
            tokens = tokens.saturating_add(match part {
                LlmContentPartIr::Text { text } => (text.chars().count() as u64).div_ceil(2),
                LlmContentPartIr::Image { .. } | LlmContentPartIr::File { .. } => 256,
            });
        }
        Ok(tokens)
    }
}

pub(super) fn fixed_request_estimate(execution: &LlmNodeExecutionSnapshot) -> u64 {
    let bytes = canonical::to_vec(&json!({
        "tools":execution.tools,
        "hostedTools":execution.hosted_tools,
        "output":execution.output,
        "request":execution.request,
    }))
    .map_or(0, |bytes| bytes.len() as u64);
    bytes.div_ceil(2).saturating_add(32)
}

pub(super) fn finalize_output(
    output: Option<&LlmOutputSpec>,
    items: &[LlmTurnItemIr],
) -> Result<LlmAttemptExecution, ApplicationError> {
    let (allow_empty, json_mode) = match output {
        Some(LlmOutputSpec::Text { allow_empty, .. }) => (*allow_empty, false),
        Some(LlmOutputSpec::Json { .. }) => (false, true),
        None => (false, false),
    };
    if json_mode {
        return Ok(finalize_failure(
            "llm_json_finalization_pending",
            "JSON output finalization is not connected yet",
        ));
    }
    let mut text = String::new();
    for item in items {
        if let LlmTurnItemIr::Message {
            role: MessageRole::Assistant,
            content,
            ..
        } = item
        {
            for part in content {
                if let LlmContentPartIr::Text { text: part } = part {
                    text.push_str(part);
                }
            }
        }
    }
    if text.is_empty() && !allow_empty {
        return Ok(finalize_failure(
            "llm_empty_output",
            "LLM response contained no assistant text",
        ));
    }
    Ok(LlmAttemptExecution::Finalize(BuiltinResult::Completed {
        outputs: [("default".into(), Value::String(text))]
            .into_iter()
            .collect(),
    }))
}

pub(super) fn assembly_failure(error: ContextAssemblyError) -> LlmAttemptExecution {
    finalize_failure(error.code, &error.message)
}

pub(super) fn known_failure(
    effect_attempt_id: &str,
    fence: &EffectAttemptFence,
    mut checkpoint: zhuangsheng_core::llm::LlmLoopCheckpoint,
    code: &str,
    message: &str,
) -> FinishModelCallCommand {
    set_model_status(&mut checkpoint, LlmLogicalCallStatus::Failed);
    let checkpoint = checkpoint.seal().expect("valid failure checkpoint");
    FinishModelCallCommand {
        effect_attempt_id: effect_attempt_id.into(),
        fence: fence.clone(),
        outcome: ModelCallEffectOutcome::Failed {
            error_bytes: canonical::to_vec(&json!({"code":code,"message":message}))
                .expect("safe error serializes"),
        },
        checkpoint,
        transcript: None,
    }
}

pub(super) fn set_model_status(
    checkpoint: &mut zhuangsheng_core::llm::LlmLoopCheckpoint,
    status: LlmLogicalCallStatus,
) {
    checkpoint.active_model_effect = checkpoint
        .active_model_effect
        .take()
        .map(|active| ActiveModelEffectCheckpoint { status, ..active });
}

pub(super) fn provider_error_bytes(error: &ProviderHttpError) -> Vec<u8> {
    canonical::to_vec(&json!({
        "code":error.code,
        "message":error.safe_message,
        "status":error.status,
        "providerRequestId":error.provider_request_id,
        "retryable":error.retryable,
        "outcomeUnknown":error.outcome_unknown,
    }))
    .unwrap_or_else(|_| b"{\"code\":\"provider_error\"}".to_vec())
}

pub(super) fn finalize_failure(code: &str, message: &str) -> LlmAttemptExecution {
    LlmAttemptExecution::Finalize(BuiltinResult::Failed {
        code: code.into(),
        safe_message: message.into(),
    })
}

pub(super) fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Ulid::new())
}
