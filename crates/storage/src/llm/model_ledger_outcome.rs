use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    canonical,
    llm::{LlmLogicalCallStatus, ModelCallEffectOutcome},
};

use crate::{StorageResult, graph::helpers::put_inline_object};

pub(super) struct StoredOutcome {
    pub logical_status: LlmLogicalCallStatus,
    pub model_status: &'static str,
    pub effect_status: &'static str,
    pub attempt_status: &'static str,
    pub result_object_id: Option<String>,
    pub error_object_id: Option<String>,
    pub usage_json: Option<String>,
    pub effect_completed: bool,
}

pub(super) async fn store_outcome<C: ConnectionTrait>(
    connection: &C,
    outcome: &ModelCallEffectOutcome,
    now: i64,
) -> StorageResult<StoredOutcome> {
    match outcome {
        ModelCallEffectOutcome::Completed {
            response_bytes,
            usage,
        } => Ok(StoredOutcome {
            logical_status: LlmLogicalCallStatus::Completed,
            model_status: "completed",
            effect_status: "succeeded",
            attempt_status: "succeeded",
            result_object_id: Some(put_inline_object(connection, response_bytes, now).await?),
            error_object_id: None,
            usage_json: usage.as_ref().map(canonical::to_string).transpose()?,
            effect_completed: true,
        }),
        ModelCallEffectOutcome::Failed { error_bytes } => Ok(StoredOutcome {
            logical_status: LlmLogicalCallStatus::Failed,
            model_status: "failed",
            effect_status: "failed",
            attempt_status: "failed",
            result_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            usage_json: None,
            effect_completed: true,
        }),
        ModelCallEffectOutcome::OutcomeUnknown { error_bytes } => Ok(StoredOutcome {
            logical_status: LlmLogicalCallStatus::OutcomeUnknown,
            model_status: "outcome_unknown",
            effect_status: "outcome_unknown",
            attempt_status: "outcome_unknown",
            result_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            usage_json: None,
            effect_completed: true,
        }),
        ModelCallEffectOutcome::RetryReady { error_bytes } => Ok(StoredOutcome {
            logical_status: LlmLogicalCallStatus::RetryReady,
            model_status: "retry_ready",
            effect_status: "pending",
            attempt_status: "outcome_unknown",
            result_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            usage_json: None,
            effect_completed: false,
        }),
    }
}
