use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{CountCallOutcome, CountResultSource, FinishCountCallCommand, LlmLogicalCallStatus},
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::put_inline_object};

use super::{
    count_ledger_helpers::append_count_event,
    count_validation::{
        CountCheckpointExpectation, FencedCountCall, load_count_attempt, validate_count_checkpoint,
        validate_count_fence, validate_count_replay_fence,
    },
    model_ledger_helpers::{add_ref, persist_checkpoint},
    validation::load_ledger_context,
};

impl SqliteStore {
    pub async fn finish_count_call(
        &self,
        command: FinishCountCallCommand,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let call =
            load_count_attempt(&transaction, &command.effect_attempt_id, &command.fence).await?;
        let context = load_ledger_context(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
        )
        .await?;
        let fingerprint = outcome_fingerprint(&command.outcome)?;
        let mut checkpoint = command.checkpoint;
        if let Some(active) = &mut checkpoint.active_count_effect {
            active.result_ref = call.result_object_id.clone();
            active.result_source = call
                .result_source
                .as_deref()
                .map(parse_source)
                .transpose()?;
        }
        checkpoint = checkpoint.seal()?;
        if is_terminal_projection(&call, &fingerprint) {
            validate_count_replay_fence(&call, &command.fence)?;
            if !payload_matches(&call, &fingerprint)
                || call.checkpoint_digest.as_deref() != Some(&checkpoint.checksum)
            {
                return Err(StorageError::Conflict("count_call_finish_replay"));
            }
            validate_count_checkpoint(
                &checkpoint,
                expectation(
                    &context,
                    &call,
                    &command.effect_attempt_id,
                    &command.fence.invoking_node_attempt_id,
                    &fingerprint,
                    call.result_object_id.as_deref(),
                ),
            )?;
            transaction.commit().await?;
            return Ok(());
        }
        validate_count_fence(&call, &command.fence)?;
        validate_fresh_state(&call, &command.outcome)?;
        let stored = store_outcome(&transaction, &command.outcome, now).await?;
        if let Some(active) = &mut checkpoint.active_count_effect {
            active.result_ref = stored.result_object_id.clone();
            active.result_source = stored.source;
        }
        checkpoint = checkpoint.seal()?;
        validate_count_checkpoint(
            &checkpoint,
            expectation(
                &context,
                &call,
                &command.effect_attempt_id,
                &command.fence.invoking_node_attempt_id,
                &fingerprint,
                stored.result_object_id.as_deref(),
            ),
        )?;
        finish_rows(
            &transaction,
            &call,
            &command.effect_attempt_id,
            &stored,
            now,
        )
        .await?;
        persist_checkpoint(&transaction, &checkpoint, now).await?;
        add_outcome_refs(
            &transaction,
            &call,
            &command.effect_attempt_id,
            &stored,
            now,
        )
        .await?;
        append_count_event(
            &transaction,
            &call.node_instance_id,
            &command.fence.invoking_node_attempt_id,
            match stored.count_status {
                "completed" => "llm.count.completed",
                "failed" => "llm.count.failed",
                _ => "llm.count.retry_ready",
            },
            json!({
                "schemaVersion":1,
                "countCallId":call.count_call_id,
                "effectId":call.effect_id,
                "effectAttemptId":command.effect_attempt_id,
                "countOrdinal":call.count_ordinal,
                "resultSource":stored.source,
                "resultRef":stored.result_object_id,
            }),
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

struct OutcomeFingerprint {
    logical_status: LlmLogicalCallStatus,
    count_status: &'static str,
    effect_status: &'static str,
    attempt_status: &'static str,
    source: Option<CountResultSource>,
    content_digest: String,
}

struct StoredCountOutcome {
    count_status: &'static str,
    effect_status: &'static str,
    attempt_status: &'static str,
    source: Option<CountResultSource>,
    result_object_id: Option<String>,
    error_object_id: Option<String>,
    effect_completed: bool,
}

fn outcome_fingerprint(outcome: &CountCallOutcome) -> StorageResult<OutcomeFingerprint> {
    Ok(match outcome {
        CountCallOutcome::Completed {
            token_count,
            source,
        } => {
            let bytes = canonical::to_vec(&count_result(*token_count, *source))?;
            OutcomeFingerprint {
                logical_status: LlmLogicalCallStatus::Completed,
                count_status: "completed",
                effect_status: "succeeded",
                attempt_status: "succeeded",
                source: Some(*source),
                content_digest: canonical::hash_bytes(&bytes),
            }
        }
        CountCallOutcome::Failed { error_bytes } => OutcomeFingerprint {
            logical_status: LlmLogicalCallStatus::Failed,
            count_status: "failed",
            effect_status: "failed",
            attempt_status: "failed",
            source: None,
            content_digest: canonical::hash_bytes(error_bytes),
        },
        CountCallOutcome::RetryReady { error_bytes } => OutcomeFingerprint {
            logical_status: LlmLogicalCallStatus::RetryReady,
            count_status: "retry_ready",
            effect_status: "pending",
            attempt_status: "outcome_unknown",
            source: None,
            content_digest: canonical::hash_bytes(error_bytes),
        },
    })
}

async fn store_outcome<C: ConnectionTrait>(
    connection: &C,
    outcome: &CountCallOutcome,
    now: i64,
) -> StorageResult<StoredCountOutcome> {
    Ok(match outcome {
        CountCallOutcome::Completed {
            token_count,
            source,
        } => StoredCountOutcome {
            count_status: "completed",
            effect_status: "succeeded",
            attempt_status: "succeeded",
            source: Some(*source),
            result_object_id: Some(
                put_inline_object(
                    connection,
                    &canonical::to_vec(&count_result(*token_count, *source))?,
                    now,
                )
                .await?,
            ),
            error_object_id: None,
            effect_completed: true,
        },
        CountCallOutcome::Failed { error_bytes } => StoredCountOutcome {
            count_status: "failed",
            effect_status: "failed",
            attempt_status: "failed",
            source: None,
            result_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            effect_completed: true,
        },
        CountCallOutcome::RetryReady { error_bytes } => StoredCountOutcome {
            count_status: "retry_ready",
            effect_status: "pending",
            attempt_status: "outcome_unknown",
            source: None,
            result_object_id: None,
            error_object_id: Some(put_inline_object(connection, error_bytes, now).await?),
            effect_completed: false,
        },
    })
}

fn count_result(token_count: u64, source: CountResultSource) -> serde_json::Value {
    json!({"schemaVersion":1,"tokenCount":token_count,"source":source})
}

fn validate_fresh_state(call: &FencedCountCall, outcome: &CountCallOutcome) -> StorageResult<()> {
    let expected_attempt = match outcome {
        CountCallOutcome::Completed {
            source: CountResultSource::Provider,
            ..
        } => "started",
        _ => {
            if call.attempt_status == "started" {
                "started"
            } else {
                "prepared"
            }
        }
    };
    let expected_count = if expected_attempt == "started" {
        "running"
    } else {
        "prepared"
    };
    if call.attempt_status != expected_attempt
        || call.effect_status != "pending"
        || call.count_status != expected_count
    {
        return Err(StorageError::Conflict("count_effect_status"));
    }
    if matches!(
        outcome,
        CountCallOutcome::Completed {
            source: CountResultSource::Provider,
            ..
        }
    ) && !call.provider_count_available
    {
        return Err(StorageError::InvalidArgument(
            "provider count result has no pinned provider operation".into(),
        ));
    }
    Ok(())
}

fn is_terminal_projection(call: &FencedCountCall, expected: &OutcomeFingerprint) -> bool {
    call.count_status == expected.count_status
        && call.effect_status == expected.effect_status
        && call.attempt_status == expected.attempt_status
}

fn payload_matches(call: &FencedCountCall, expected: &OutcomeFingerprint) -> bool {
    if expected.logical_status == LlmLogicalCallStatus::Completed {
        call.result_digest.as_deref() == Some(&expected.content_digest)
            && call.result_source.as_deref() == expected.source.map(source_name)
            && call.error_digest.is_none()
    } else {
        call.result_object_id.is_none()
            && call.error_digest.as_deref() == Some(&expected.content_digest)
            && call.result_source.is_none()
    }
}

fn expectation<'a>(
    context: &'a super::validation::LedgerContext,
    call: &'a FencedCountCall,
    effect_attempt_id: &'a str,
    updater_attempt_id: &'a str,
    outcome: &OutcomeFingerprint,
    result_ref: Option<&'a str>,
) -> CountCheckpointExpectation<'a> {
    CountCheckpointExpectation {
        context,
        node_instance_id: &call.node_instance_id,
        updater_attempt_id,
        count_call_id: &call.count_call_id,
        effect_id: &call.effect_id,
        effect_attempt_id,
        count_ordinal: call.count_ordinal,
        pin_digest: &call.pin_digest,
        candidate_ref: &call.candidate_ref,
        candidate_digest: &call.candidate_digest,
        request_digest: &call.request_digest,
        status: outcome.logical_status,
        result_source: outcome.source,
        result_ref,
    }
}

async fn finish_rows<C: ConnectionTrait>(
    connection: &C,
    call: &FencedCountCall,
    effect_attempt_id: &str,
    outcome: &StoredCountOutcome,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute(crate::graph::helpers::sql(
        "UPDATE effect_attempts SET status = ?, result_object_id = ?, error_object_id = ?, finished_at = ? WHERE id = ? AND status IN ('prepared','started')",
        vec![outcome.attempt_status.into(), outcome.result_object_id.clone().into(), outcome.error_object_id.clone().into(), now.into(), effect_attempt_id.into()],
    )).await?;
    let effect = connection.execute(crate::graph::helpers::sql(
        "UPDATE effects SET status = ?, result_object_id = ?, completed_at = ? WHERE id = ? AND status = 'pending'",
        vec![outcome.effect_status.into(), outcome.result_object_id.clone().into(), outcome.effect_completed.then_some(now).into(), call.effect_id.clone().into()],
    )).await?;
    let count = connection.execute(crate::graph::helpers::sql(
        "UPDATE count_calls SET status = ?, result_source = ?, result_object_id = ?, finished_at = ? WHERE id = ? AND status IN ('prepared','running')",
        vec![outcome.count_status.into(), outcome.source.map(source_name).into(), outcome.result_object_id.clone().into(), now.into(), call.count_call_id.clone().into()],
    )).await?;
    if attempt.rows_affected() != 1 || effect.rows_affected() != 1 || count.rows_affected() != 1 {
        return Err(StorageError::Conflict("count_effect_terminal_status"));
    }
    Ok(())
}

async fn add_outcome_refs<C: ConnectionTrait>(
    connection: &C,
    call: &FencedCountCall,
    effect_attempt_id: &str,
    outcome: &StoredCountOutcome,
    now: i64,
) -> StorageResult<()> {
    if let Some(object_id) = &outcome.result_object_id {
        for (kind, id) in [
            ("count_call", call.count_call_id.as_str()),
            ("effect", call.effect_id.as_str()),
            ("effect_attempt", effect_attempt_id),
        ] {
            add_ref(connection, object_id, kind, id, "result", now).await?;
        }
    }
    if let Some(object_id) = &outcome.error_object_id {
        add_ref(
            connection,
            object_id,
            "effect_attempt",
            effect_attempt_id,
            "error",
            now,
        )
        .await?;
    }
    Ok(())
}

fn source_name(source: CountResultSource) -> &'static str {
    match source {
        CountResultSource::Provider => "provider",
        CountResultSource::Local => "local",
        CountResultSource::Estimate => "estimate",
    }
}

fn parse_source(value: &str) -> StorageResult<CountResultSource> {
    match value {
        "provider" => Ok(CountResultSource::Provider),
        "local" => Ok(CountResultSource::Local),
        "estimate" => Ok(CountResultSource::Estimate),
        _ => Err(StorageError::Integrity(
            "unknown count result source".into(),
        )),
    }
}
