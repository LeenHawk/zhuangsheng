use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::llm::{EffectResolutionKind, ResolveEffectUnknownCommand};

use crate::{
    StorageResult,
    runtime::{Event, append_event},
};

use super::effect_resolution_helpers::{ResolutionContext, ResolutionOwner};

pub(super) async fn append_resolution_events<C: ConnectionTrait>(
    connection: &C,
    command: &ResolveEffectUnknownCommand,
    context: &ResolutionContext,
    now: i64,
) -> StorageResult<()> {
    let (effect_event, owner_event) = match (&context.owner, command.kind) {
        (ResolutionOwner::Model(_), EffectResolutionKind::ConfirmSucceeded) => {
            ("effect.succeeded", "llm.call.completed")
        }
        (ResolutionOwner::Tool(_), EffectResolutionKind::ConfirmSucceeded) => {
            ("effect.succeeded", "tool.call.completed")
        }
        (ResolutionOwner::Model(_), EffectResolutionKind::ConfirmFailedRetrySafe) => {
            ("effect.retry_ready", "llm.call.retry_ready")
        }
        (ResolutionOwner::Tool(_), EffectResolutionKind::ConfirmFailedRetrySafe) => {
            ("effect.retry_ready", "tool.call.retry_ready")
        }
        (ResolutionOwner::Model(_), EffectResolutionKind::AbortRun) => {
            ("effect.abandoned_unknown", "llm.call.failed")
        }
        (ResolutionOwner::Tool(_), EffectResolutionKind::AbortRun) => {
            ("effect.abandoned_unknown", "tool.call.failed")
        }
    };
    let payload = json!({
        "schemaVersion":1,
        "effectId":command.effect_id,
        "effectAttemptId":command.expected_effect_attempt_id,
        "ownerId":context.owner.id(),
        "resolutionId":command.resolution_id,
        "resolutionKind":command.kind,
        "resultRef":command.result_object_id,
    });
    for event_type in [effect_event, owner_event] {
        append_event(
            connection,
            Event {
                run_id: &context.run_id,
                event_type,
                importance: "critical",
                node_instance_id: Some(&context.node_instance_id),
                attempt_id: Some(&context.invoking_node_attempt_id),
                payload: payload.clone(),
                now,
            },
        )
        .await?;
    }
    Ok(())
}
