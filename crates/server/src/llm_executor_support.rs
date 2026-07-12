use serde_json::json;
use ulid::Ulid;
use zhuangsheng_core::{
    canonical,
    graph::LlmNodeExecutionSnapshot,
    llm::{
        ActiveModelEffectCheckpoint, LlmLogicalCallStatus,
        context::{ContextAssemblyError, ContextAssemblyResult, ContextRole, ContextTokenCounter},
        ir::LlmContentPartIr,
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
        "toolDescriptors":execution.tool_descriptors,
        "hostedTools":execution.hosted_tools,
        "output":execution.output,
        "request":execution.request,
    }))
    .map_or(0, |bytes| bytes.len() as u64);
    bytes.div_ceil(2).saturating_add(32)
}

pub(super) fn assembly_failure(error: ContextAssemblyError) -> LlmAttemptExecution {
    finalize_failure(error.code, &error.message)
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
