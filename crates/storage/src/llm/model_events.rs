use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::llm::LlmLogicalCallStatus;

use crate::{
    StorageError, StorageResult,
    graph::helpers::sql,
    runtime::{Event, append_event},
};

use super::model_ledger_outcome::StoredOutcome;

pub(super) async fn append_model_event<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
    node_attempt_id: &str,
    event_type: &str,
    payload: Value,
    now: i64,
) -> StorageResult<()> {
    let row = connection
        .query_one_raw(sql(
            "SELECT run_id FROM node_instances WHERE id = ?",
            vec![node_instance_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("model event owner is unavailable".into()))?;
    let run_id: String = row.try_get("", "run_id")?;
    append_event(
        connection,
        Event {
            run_id: &run_id,
            event_type,
            importance: "critical",
            node_instance_id: Some(node_instance_id),
            attempt_id: Some(node_attempt_id),
            payload,
            now,
        },
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn append_model_outcome_events<C: ConnectionTrait>(
    connection: &C,
    node_instance_id: &str,
    node_attempt_id: &str,
    model_call_id: &str,
    effect_id: &str,
    effect_attempt_id: &str,
    stored: &StoredOutcome,
    wait_id: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    let payload = json!({
        "schemaVersion":1,
        "modelCallId":model_call_id,
        "effectId":effect_id,
        "effectAttemptId":effect_attempt_id,
        "resultRef":stored.result_object_id,
        "waitId":wait_id,
    });
    let (call_event, effect_event) = match stored.logical_status {
        LlmLogicalCallStatus::Completed => ("llm.call.completed", "effect.succeeded"),
        LlmLogicalCallStatus::Failed => ("llm.call.failed", "effect.failed"),
        LlmLogicalCallStatus::OutcomeUnknown => (
            "llm.call.outcome_unknown",
            "effect.outcome_unknown.recorded",
        ),
        LlmLogicalCallStatus::RetryReady => {
            ("llm.call.retry_ready", "effect.attempt.outcome_unknown")
        }
        _ => {
            return Err(StorageError::Integrity(
                "model outcome event status is not terminal".into(),
            ));
        }
    };
    append_model_event(
        connection,
        node_instance_id,
        node_attempt_id,
        effect_event,
        payload.clone(),
        now,
    )
    .await?;
    append_model_event(
        connection,
        node_instance_id,
        node_attempt_id,
        call_event,
        payload,
        now,
    )
    .await
}
