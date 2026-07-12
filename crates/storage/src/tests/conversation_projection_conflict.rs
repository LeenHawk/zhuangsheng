use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        context::CommitContextPatchCommand,
        conversation::{CreateConversationCommand, SubmitConversationTurnCommand},
    },
    llm::ir::LlmContentPartIr,
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{graph::helpers::sql, tests::store};

use super::{
    conversation_projection_support::complete_with_reply,
    conversation_run_profile::{compatible_revision, run_spec},
};

const NOW: i64 = 1_700_000_400_000;

#[tokio::test]
async fn projector_classifies_candidate_head_mismatch_as_terminal_conflict() {
    let store = store().await;
    let revision_id = compatible_revision(&store, "projection-conflict").await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "projection-conflict-conversation".into(),
            },
            NOW,
        )
        .await
        .unwrap();
    let submitted = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id,
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: vec![LlmContentPartIr::Text {
                    text: "conflict".into(),
                }],
                run: run_spec(&revision_id),
                idempotency_key: "projection-conflict-turn".into(),
            },
            NOW + 1,
        )
        .await
        .unwrap();
    complete_with_reply(
        &store,
        &submitted.run.id,
        &submitted.turn.user_commit_id,
        NOW + 2,
    )
    .await;
    store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: submitted.run.context_id.clone(),
                lineage_key: submitted.candidate.branch_id,
                base_commit_id: submitted.turn.user_commit_id,
                operation_id: "projection-conflict-advance".into(),
                ops: vec![JsonPatchOp::Add {
                    path: "/testMarker".into(),
                    value: json!(true),
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::System,
                    id: None,
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .maintain_candidate_projections(NOW + 3, "projector-conflict", 10)
            .await
            .unwrap(),
        1
    );
    let row = store.db.query_one_raw(sql(
        "SELECT tc.status, j.status AS job_status, tc.assistant_message_id FROM turn_candidates tc JOIN candidate_projection_jobs j ON j.run_id = tc.run_id WHERE tc.run_id = ?",
        vec![submitted.run.id.into()],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "status").unwrap(),
        "projection_conflicted"
    );
    assert_eq!(
        row.try_get::<String>("", "job_status").unwrap(),
        "conflicted"
    );
    assert!(
        row.try_get::<Option<String>>("", "assistant_message_id")
            .unwrap()
            .is_none()
    );
}
