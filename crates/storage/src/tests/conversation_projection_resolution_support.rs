use serde_json::json;
use zhuangsheng_core::{
    application::{
        context::CommitContextPatchCommand,
        conversation::{CreateConversationCommand, SubmitConversationTurnCommand},
    },
    llm::ir::LlmContentPartIr,
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::SqliteStore;

use super::{
    conversation_projection_support::complete_with_reply,
    conversation_run_profile::{compatible_revision, run_spec},
};

pub(super) struct ConflictFixture {
    pub turn_id: String,
    pub run_id: String,
    pub branch_id: String,
    pub output_commit_id: String,
    pub current_head: String,
}

pub(super) async fn conflicted_candidate(
    store: &SqliteStore,
    key: &str,
    now: i64,
) -> ConflictFixture {
    let revision_id = compatible_revision(store, &format!("resolution-{key}")).await;
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: format!("resolution-conversation-{key}"),
            },
            now,
        )
        .await
        .unwrap();
    let submitted = store
        .submit_conversation_turn_at(
            SubmitConversationTurnCommand {
                conversation_id: conversation.id.clone(),
                expected_head_commit_id: conversation.active_head_commit_id,
                user_content: vec![LlmContentPartIr::Text {
                    text: "resolve this reply".into(),
                }],
                run: run_spec(&revision_id),
                idempotency_key: format!("resolution-turn-{key}"),
            },
            now + 1,
        )
        .await
        .unwrap();
    complete_with_reply(
        store,
        &submitted.run.id,
        &submitted.turn.user_commit_id,
        now + 2,
    )
    .await;
    let current_head = store
        .commit_context_patch(CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: submitted.run.context_id,
                lineage_key: submitted.candidate.branch_id.clone(),
                base_commit_id: submitted.turn.user_commit_id.clone(),
                operation_id: format!("resolution-advance-{key}"),
                ops: vec![JsonPatchOp::Add {
                    path: "/resolutionMarker".into(),
                    value: json!(key),
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
        .unwrap()
        .id;
    assert_eq!(
        store
            .maintain_candidate_projections(now + 3, &format!("resolver-{key}"), 10)
            .await
            .unwrap(),
        1
    );
    ConflictFixture {
        turn_id: submitted.turn.id,
        run_id: submitted.run.id,
        branch_id: submitted.candidate.branch_id,
        output_commit_id: submitted.turn.user_commit_id,
        current_head,
    }
}
