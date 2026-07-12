use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    canonical,
    llm::{LlmLogicalCallStatus, ModelCallEffectOutcome, PrepareModelCallRetryCommand},
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::validation::FencedModelCall;

pub(super) enum ReplayDecision {
    Fresh,
    Replayed,
}

pub(super) struct FinishReplay {
    pub decision: ReplayDecision,
    pub logical_status: LlmLogicalCallStatus,
    pub response_ref: Option<String>,
}

struct OutcomeProjection {
    logical_status: LlmLogicalCallStatus,
    model_status: &'static str,
    effect_status: &'static str,
    attempt_status: &'static str,
    content_digest: String,
    usage: Option<String>,
}

pub(super) struct RetryReplay {
    pub node_instance_id: String,
    pub call_no: u64,
    pub effect_id: String,
}

pub(super) async fn load_retry_replay<C: ConnectionTrait>(
    connection: &C,
    command: &PrepareModelCallRetryCommand,
) -> StorageResult<Option<RetryReplay>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT ea.id AS effect_attempt_id, ea.invoking_node_attempt_id, ea.status AS attempt_status, ea.request_object_id AS attempt_request_object_id, e.id AS effect_id, e.status AS effect_status, mc.node_instance_id, mc.call_no, mc.status AS model_status, mc.request_object_id AS model_request_object_id, cp.checkpoint_digest FROM effects e JOIN model_calls mc ON mc.id = e.model_call_id JOIN effect_attempts ea ON ea.effect_id = e.id LEFT JOIN llm_loop_checkpoints cp ON cp.node_instance_id = mc.node_instance_id WHERE mc.id = ? AND (ea.id = ? OR ea.invoking_node_attempt_id = ?) ORDER BY ea.attempt_no DESC LIMIT 1",
            vec![
                command.model_call_id.clone().into(),
                command.effect_attempt_id.clone().into(),
                command.fence.invoking_node_attempt_id.clone().into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let attempt_id: String = row.try_get("", "effect_attempt_id")?;
    let invoker: String = row.try_get("", "invoking_node_attempt_id")?;
    let matches = attempt_id == command.effect_attempt_id
        && invoker == command.fence.invoking_node_attempt_id
        && row.try_get::<String>("", "attempt_status")? == "prepared"
        && row.try_get::<String>("", "effect_status")? == "pending"
        && row.try_get::<String>("", "model_status")? == "prepared"
        && row.try_get::<String>("", "attempt_request_object_id")?
            == row.try_get::<String>("", "model_request_object_id")?
        && row
            .try_get::<Option<String>>("", "checkpoint_digest")?
            .as_deref()
            == Some(&command.checkpoint.checksum);
    if !matches {
        return Err(StorageError::Conflict("model_call_retry_replay"));
    }
    Ok(Some(RetryReplay {
        node_instance_id: row.try_get("", "node_instance_id")?,
        call_no: u64::try_from(row.try_get::<i64>("", "call_no")?)
            .map_err(|_| StorageError::Integrity("invalid model call number".into()))?,
        effect_id: row.try_get("", "effect_id")?,
    }))
}

pub(super) fn classify_start(
    fenced: &FencedModelCall,
    provider_request_id: &Option<String>,
    checkpoint_digest: &str,
) -> StorageResult<ReplayDecision> {
    match (
        fenced.attempt_status.as_str(),
        fenced.effect_status.as_str(),
        fenced.model_status.as_str(),
    ) {
        ("prepared", "pending", "prepared") => Ok(ReplayDecision::Fresh),
        ("started", "pending", "running")
            if &fenced.attempt_provider_request_id == provider_request_id
                && &fenced.model_provider_request_id == provider_request_id
                && fenced.checkpoint_digest.as_deref() == Some(checkpoint_digest) =>
        {
            Ok(ReplayDecision::Replayed)
        }
        ("started", "pending", "running") => Err(StorageError::Conflict("model_call_start_replay")),
        _ => Err(StorageError::Conflict("model_effect_status")),
    }
}

pub(super) fn classify_finish(
    fenced: &FencedModelCall,
    outcome: &ModelCallEffectOutcome,
    checkpoint_digest: &str,
) -> StorageResult<FinishReplay> {
    let projection = outcome_projection(outcome)?;
    if fenced.attempt_status == "started"
        && fenced.effect_status == "pending"
        && fenced.model_status == "running"
    {
        return Ok(FinishReplay {
            decision: ReplayDecision::Fresh,
            logical_status: projection.logical_status,
            response_ref: None,
        });
    }
    let state_matches = fenced.model_status == projection.model_status
        && fenced.effect_status == projection.effect_status
        && fenced.attempt_status == projection.attempt_status;
    let payload_matches = if projection.logical_status == LlmLogicalCallStatus::Completed {
        fenced.response_digest.as_deref() == Some(&projection.content_digest)
            && fenced.attempt_result_digest.as_deref() == Some(&projection.content_digest)
            && fenced.attempt_error_digest.is_none()
            && fenced.usage_json == projection.usage
            && fenced.response_object_id == fenced.attempt_result_object_id
    } else {
        fenced.response_object_id.is_none()
            && fenced.attempt_result_object_id.is_none()
            && fenced.attempt_error_digest.as_deref() == Some(&projection.content_digest)
            && fenced.usage_json.is_none()
    };
    if state_matches
        && payload_matches
        && fenced.checkpoint_digest.as_deref() == Some(checkpoint_digest)
    {
        return Ok(FinishReplay {
            decision: ReplayDecision::Replayed,
            logical_status: projection.logical_status,
            response_ref: fenced.response_object_id.clone(),
        });
    }
    if state_matches {
        Err(StorageError::Conflict("model_call_finish_replay"))
    } else {
        Err(StorageError::Conflict("model_effect_status"))
    }
}

fn outcome_projection(outcome: &ModelCallEffectOutcome) -> StorageResult<OutcomeProjection> {
    Ok(match outcome {
        ModelCallEffectOutcome::Completed {
            response_bytes,
            usage,
        } => OutcomeProjection {
            logical_status: LlmLogicalCallStatus::Completed,
            model_status: "completed",
            effect_status: "succeeded",
            attempt_status: "succeeded",
            content_digest: canonical::hash_bytes(response_bytes),
            usage: usage.as_ref().map(canonical::to_string).transpose()?,
        },
        ModelCallEffectOutcome::Failed { error_bytes } => OutcomeProjection {
            logical_status: LlmLogicalCallStatus::Failed,
            model_status: "failed",
            effect_status: "failed",
            attempt_status: "failed",
            content_digest: canonical::hash_bytes(error_bytes),
            usage: None,
        },
        ModelCallEffectOutcome::OutcomeUnknown { error_bytes } => OutcomeProjection {
            logical_status: LlmLogicalCallStatus::OutcomeUnknown,
            model_status: "outcome_unknown",
            effect_status: "outcome_unknown",
            attempt_status: "outcome_unknown",
            content_digest: canonical::hash_bytes(error_bytes),
            usage: None,
        },
        ModelCallEffectOutcome::RetryReady { error_bytes } => OutcomeProjection {
            logical_status: LlmLogicalCallStatus::RetryReady,
            model_status: "retry_ready",
            effect_status: "pending",
            attempt_status: "outcome_unknown",
            content_digest: canonical::hash_bytes(error_bytes),
            usage: None,
        },
    })
}
