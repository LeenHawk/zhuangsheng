use std::collections::BTreeMap;

use zhuangsheng_core::{
    application::memory::{ListMemoryProposalsCommand, ProposeMemoryChangeCommand},
    memory::{LongTermMemoryContentV1, MemoryProposalChangeInput, MemoryProposalStatus},
    state::{ActorKind, ActorRef},
};

use super::store;

#[tokio::test]
async fn proposal_inbox_is_cursor_paginated_and_includes_review_content() {
    let store = store().await;
    for (key, text) in [("proposal-a", "First fact"), ("proposal-b", "Second fact")] {
        store
            .propose_memory_change(command(key, text))
            .await
            .unwrap();
    }
    let first = store
        .list_memory_proposals(ListMemoryProposalsCommand {
            scope_id: "roleplay".into(),
            status: Some(MemoryProposalStatus::AwaitingReview),
            limit: 1,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(first.proposals.len(), 1);
    assert!(first.proposals[0].proposed_content.is_some());
    let second = store
        .list_memory_proposals(ListMemoryProposalsCommand {
            scope_id: "roleplay".into(),
            status: Some(MemoryProposalStatus::AwaitingReview),
            limit: 1,
            cursor: first.next_cursor,
        })
        .await
        .unwrap();
    assert_eq!(second.proposals.len(), 1);
    assert_ne!(first.proposals[0].id, second.proposals[0].id);
    assert!(second.next_cursor.is_none());
}

fn command(key: &str, text: &str) -> ProposeMemoryChangeCommand {
    ProposeMemoryChangeCommand {
        scope_id: "roleplay".into(),
        memory_id: None,
        expected_head_commit_id: None,
        change: MemoryProposalChangeInput::Create {
            content: LongTermMemoryContentV1 {
                schema_version: 1,
                text: text.into(),
                tags: vec!["story".into()],
                attributes: BTreeMap::new(),
            },
        },
        reason: "remember story fact".into(),
        evidence_refs: vec!["message:1".into()],
        requested_by: ActorRef {
            kind: ActorKind::User,
            id: Some("user-1".into()),
        },
        idempotency_key: key.into(),
        schema_version: 1,
        policy_version: 1,
        origin_run_id: None,
        origin_node_instance_id: None,
    }
}
