use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::conversation::{CandidateProjectionResolution, ResolveCandidateProjectionCommand},
    conversation::TurnCandidateStatus,
};

use crate::{StorageError, graph::helpers::sql, tests::store};

use super::conversation_projection_resolution_support::conflicted_candidate;

const NOW: i64 = 1_700_000_420_000;

#[tokio::test]
async fn append_after_current_reuses_output_and_preserves_advanced_state() {
    let store = store().await;
    let fixture = conflicted_candidate(&store, "append", NOW).await;
    let command = ResolveCandidateProjectionCommand {
        turn_id: fixture.turn_id.clone(),
        run_id: fixture.run_id.clone(),
        expected_current_branch_head: fixture.current_head.clone(),
        resolution: CandidateProjectionResolution::AppendAfterCurrent {
            reason: "The intervening world-state change is compatible.".into(),
        },
        idempotency_key: "projection-resolution-append".into(),
    };
    let resolved = store
        .resolve_candidate_projection_at(command.clone(), NOW + 4)
        .await
        .unwrap();
    assert_eq!(resolved.status, TurnCandidateStatus::Ready);
    assert_eq!(resolved.branch_id, fixture.branch_id);
    assert_ne!(resolved.branch_head_commit_id, fixture.current_head);
    assert!(resolved.assistant_message_id.is_some());
    assert_eq!(
        store
            .resolve_candidate_projection_at(command.clone(), NOW + 5)
            .await
            .unwrap(),
        resolved
    );
    let conflicting_reuse = store
        .resolve_candidate_projection_at(
            ResolveCandidateProjectionCommand {
                turn_id: fixture.turn_id.clone(),
                run_id: fixture.run_id.clone(),
                expected_current_branch_head: fixture.current_head.clone(),
                resolution: CandidateProjectionResolution::AbandonProjection {
                    reason: "different request".into(),
                },
                idempotency_key: command.idempotency_key,
            },
            NOW + 5,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        conflicting_reuse,
        StorageError::IdempotencyConflict
    ));
    let context = store
        .get_context_at_commit(&resolved.branch_head_commit_id)
        .await
        .unwrap();
    assert_eq!(context.value["resolutionMarker"], "append");
    assert_eq!(context.value["messages"].as_array().unwrap().len(), 2);
    let row = store.db.query_one_raw(sql(
        "SELECT tc.status, tc.projection_error_object_id, j.status AS job_status, c.active_head_commit_id FROM turn_candidates tc JOIN candidate_projection_jobs j ON j.run_id = tc.run_id JOIN conversation_turns t ON t.id = tc.turn_id JOIN conversations c ON c.id = t.conversation_id WHERE tc.run_id = ?",
        vec![fixture.run_id.clone().into()],
    )).await.unwrap().unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "ready");
    assert_eq!(
        row.try_get::<String>("", "job_status").unwrap(),
        "conflicted"
    );
    assert!(
        row.try_get::<Option<String>>("", "projection_error_object_id")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        row.try_get::<String>("", "active_head_commit_id").unwrap(),
        resolved.branch_head_commit_id
    );
    let audit = store.db.query_one_raw(sql(
        "SELECT payload_json FROM domain_events WHERE aggregate_id = (SELECT conversation_id FROM conversation_turns WHERE id = ?) AND event_type = 'conversation.candidate_projection_resolved'",
        vec![fixture.turn_id.into()],
    )).await.unwrap().unwrap().try_get::<String>("", "payload_json").unwrap();
    assert!(audit.contains("intervening world-state change"));
}

#[tokio::test]
async fn abandon_projection_keeps_branch_and_creates_no_assistant_message() {
    let store = store().await;
    let fixture = conflicted_candidate(&store, "abandon", NOW + 100).await;
    let resolved = store
        .resolve_candidate_projection_at(
            ResolveCandidateProjectionCommand {
                turn_id: fixture.turn_id.clone(),
                run_id: fixture.run_id.clone(),
                expected_current_branch_head: fixture.current_head.clone(),
                resolution: CandidateProjectionResolution::AbandonProjection {
                    reason: "The story has intentionally moved on.".into(),
                },
                idempotency_key: "projection-resolution-abandon".into(),
            },
            NOW + 104,
        )
        .await
        .unwrap();
    assert_eq!(resolved.status, TurnCandidateStatus::ProjectionAbandoned);
    assert_eq!(resolved.branch_head_commit_id, fixture.current_head);
    assert_eq!(resolved.assistant_message_id, None);
    let row = store.db.query_one_raw(sql(
        "SELECT status, assistant_message_id, candidate_commit_id FROM turn_candidates WHERE run_id = ?",
        vec![fixture.run_id.into()],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "status").unwrap(),
        "projection_abandoned"
    );
    assert!(
        row.try_get::<Option<String>>("", "assistant_message_id")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn resolution_rejects_a_stale_current_head() {
    let store = store().await;
    let fixture = conflicted_candidate(&store, "stale", NOW + 200).await;
    let run_id = fixture.run_id.clone();
    let error = store
        .resolve_candidate_projection_at(
            ResolveCandidateProjectionCommand {
                turn_id: fixture.turn_id,
                run_id: fixture.run_id,
                expected_current_branch_head: fixture.output_commit_id,
                resolution: CandidateProjectionResolution::AbandonProjection {
                    reason: "stale operator view".into(),
                },
                idempotency_key: "projection-resolution-stale".into(),
            },
            NOW + 204,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::Conflict("candidate_branch_head")
    ));
    let status = store
        .db
        .query_one_raw(sql(
            "SELECT status FROM turn_candidates WHERE run_id = ?",
            vec![run_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "status")
        .unwrap();
    assert_eq!(status, "projection_conflicted");
}
