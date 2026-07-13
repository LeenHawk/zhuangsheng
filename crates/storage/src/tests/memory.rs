use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::memory::{
        ApplyMemoryProposalCommand, DecideMemoryProposalCommand, ListMemoryProposalsCommand,
        MemoryProposalDecision, MemorySearchCommand, ProposeMemoryChangeCommand,
    },
    memory::{
        LongTermMemoryContentV1, LongTermMemoryStatus, MemoryProposalChangeInput,
        MemoryProposalStatus,
    },
    state::{ActorKind, ActorRef},
};

use crate::{
    StorageError,
    graph::helpers::{now_ms, sql},
};

use super::store;

#[tokio::test]
async fn create_proposal_review_apply_and_search_are_durable() {
    let store = store().await;
    let command = create_command("create-dragon", "Dragons guard the northern gate");
    let proposed = store.propose_memory_change(command.clone()).await.unwrap();
    assert_eq!(proposed.status, MemoryProposalStatus::AwaitingReview);
    assert_eq!(
        store.propose_memory_change(command).await.unwrap(),
        proposed
    );
    assert_eq!(
        store
            .get_memory_record(&proposed.memory_id)
            .await
            .unwrap()
            .status,
        LongTermMemoryStatus::Reserved
    );
    let approved = approve(&store, &proposed.id, "approve-dragon").await;
    assert_eq!(approved.status, MemoryProposalStatus::Approved);
    let applied = apply(&store, &proposed.id, "apply-dragon").await;
    assert_eq!(applied.status, MemoryProposalStatus::Applied);
    assert!(applied.applied_commit_id.is_some());
    let record = store.get_memory_record(&proposed.memory_id).await.unwrap();
    assert_eq!(record.status, LongTermMemoryStatus::Active);
    assert_eq!(record.content.as_ref().unwrap().tags, ["lore", "story"]);

    let search = store
        .search_memory(MemorySearchCommand {
            scope_id: "roleplay".into(),
            text: Some("dragons northern".into()),
            tags: vec!["story".into()],
            status: None,
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(search.records.len(), 1);
    assert_eq!(search.records[0].id, proposed.memory_id);
    assert_eq!(
        search.scope_snapshot_token,
        "memory-scope:roleplay:revision:1"
    );
    assert!(!search.truncated);
    assert_eq!(count(&store, "memory_proposal_transitions").await, 4);
    assert_eq!(count(&store, "domain_events").await, 1);
    store
        .maintain_content_objects(now_ms() + 60_001, 60_000, 1_000)
        .await
        .unwrap();
    let after_gc = store.get_memory_record(&proposed.memory_id).await.unwrap();
    assert_eq!(
        after_gc.content.unwrap().text,
        "Dragons guard the northern gate"
    );
    let proposals = store
        .list_memory_proposals(ListMemoryProposalsCommand {
            scope_id: "roleplay".into(),
            status: Some(MemoryProposalStatus::Applied),
            limit: 10,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(proposals.proposals[0].evidence_refs, ["message:1"]);
}

#[tokio::test]
async fn concurrent_approved_proposal_conflicts_instead_of_overwriting_head() {
    let store = store().await;
    let created = store
        .propose_memory_change(create_command("create-base", "Original fact"))
        .await
        .unwrap();
    approve(&store, &created.id, "approve-base").await;
    apply(&store, &created.id, "apply-base").await;
    let record = store.get_memory_record(&created.memory_id).await.unwrap();
    let head = record.head_commit_id.clone().unwrap();
    let first = store
        .propose_memory_change(replace_command(
            "replace-first",
            &created.memory_id,
            &head,
            "First replacement",
        ))
        .await
        .unwrap();
    let second = store
        .propose_memory_change(replace_command(
            "replace-second",
            &created.memory_id,
            &head,
            "Second replacement",
        ))
        .await
        .unwrap();
    approve(&store, &first.id, "approve-first").await;
    approve(&store, &second.id, "approve-second").await;
    apply(&store, &first.id, "apply-first").await;
    let conflicted = apply(&store, &second.id, "apply-second").await;
    assert_eq!(conflicted.status, MemoryProposalStatus::Conflicted);
    let current = store.get_memory_record(&created.memory_id).await.unwrap();
    assert_eq!(current.content.unwrap().text, "First replacement");
    assert_ne!(current.head_commit_id.unwrap(), head);
}

#[tokio::test]
async fn rejected_create_discards_reserved_identity_and_receipt_conflicts() {
    let store = store().await;
    let proposed = store
        .propose_memory_change(create_command("reject-create", "Temporary claim"))
        .await
        .unwrap();
    let rejected = store
        .decide_memory_proposal(DecideMemoryProposalCommand {
            proposal_id: proposed.id,
            expected_status: MemoryProposalStatus::AwaitingReview,
            decision: MemoryProposalDecision::Reject,
            actor: actor(),
            idempotency_key: "reject-command".into(),
        })
        .await
        .unwrap();
    assert_eq!(rejected.status, MemoryProposalStatus::Rejected);
    assert_eq!(
        store
            .get_memory_record(&rejected.memory_id)
            .await
            .unwrap()
            .status,
        LongTermMemoryStatus::Discarded
    );
    let mut conflicting = create_command("reject-create", "Different claim");
    conflicting.reason = "different request".into();
    assert!(matches!(
        store.propose_memory_change(conflicting).await.unwrap_err(),
        StorageError::IdempotencyConflict
    ));
}

#[tokio::test]
async fn obsolete_and_tombstone_transitions_update_search_projection() {
    let store = store().await;
    let created = store
        .propose_memory_change(create_command("create-lifecycle", "Ancient rumor"))
        .await
        .unwrap();
    approve(&store, &created.id, "approve-lifecycle").await;
    apply(&store, &created.id, "apply-lifecycle").await;
    let active = store.get_memory_record(&created.memory_id).await.unwrap();
    let obsolete = store
        .propose_memory_change(lifecycle_command(
            "obsolete-lifecycle",
            &created.memory_id,
            active.head_commit_id.as_deref().unwrap(),
            MemoryProposalChangeInput::MarkObsolete,
        ))
        .await
        .unwrap();
    approve(&store, &obsolete.id, "approve-obsolete").await;
    apply(&store, &obsolete.id, "apply-obsolete").await;
    let obsolete_record = store.get_memory_record(&created.memory_id).await.unwrap();
    assert_eq!(obsolete_record.status, LongTermMemoryStatus::Obsolete);
    let obsolete_search = store
        .search_memory(MemorySearchCommand {
            scope_id: "roleplay".into(),
            text: Some("ancient".into()),
            tags: vec![],
            status: Some(LongTermMemoryStatus::Obsolete),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(obsolete_search.records.len(), 1);

    let deleted = store
        .propose_memory_change(lifecycle_command(
            "delete-lifecycle",
            &created.memory_id,
            obsolete_record.head_commit_id.as_deref().unwrap(),
            MemoryProposalChangeInput::DeleteTombstone,
        ))
        .await
        .unwrap();
    approve(&store, &deleted.id, "approve-delete").await;
    apply(&store, &deleted.id, "apply-delete").await;
    let record = store.get_memory_record(&created.memory_id).await.unwrap();
    assert_eq!(record.status, LongTermMemoryStatus::Deleted);
    assert!(record.content.is_none());
    assert!(
        store
            .search_memory(MemorySearchCommand {
                scope_id: "roleplay".into(),
                text: Some("ancient".into()),
                tags: vec![],
                status: Some(LongTermMemoryStatus::Obsolete),
                limit: 10,
            })
            .await
            .unwrap()
            .records
            .is_empty()
    );
}

fn create_command(key: &str, text: &str) -> ProposeMemoryChangeCommand {
    ProposeMemoryChangeCommand {
        scope_id: "roleplay".into(),
        memory_id: None,
        expected_head_commit_id: None,
        change: MemoryProposalChangeInput::Create {
            content: content(text),
        },
        reason: "remember role-play lore".into(),
        evidence_refs: vec!["message:1".into()],
        requested_by: actor(),
        idempotency_key: key.into(),
        schema_version: 1,
        policy_version: 1,
        origin_run_id: None,
        origin_node_instance_id: None,
    }
}

fn replace_command(
    key: &str,
    memory_id: &str,
    head: &str,
    text: &str,
) -> ProposeMemoryChangeCommand {
    ProposeMemoryChangeCommand {
        scope_id: "roleplay".into(),
        memory_id: Some(memory_id.into()),
        expected_head_commit_id: Some(head.into()),
        change: MemoryProposalChangeInput::ReplaceContent {
            content: content(text),
        },
        reason: "replace outdated lore".into(),
        evidence_refs: vec!["message:2".into()],
        requested_by: actor(),
        idempotency_key: key.into(),
        schema_version: 1,
        policy_version: 1,
        origin_run_id: None,
        origin_node_instance_id: None,
    }
}

fn lifecycle_command(
    key: &str,
    memory_id: &str,
    head: &str,
    change: MemoryProposalChangeInput,
) -> ProposeMemoryChangeCommand {
    ProposeMemoryChangeCommand {
        scope_id: "roleplay".into(),
        memory_id: Some(memory_id.into()),
        expected_head_commit_id: Some(head.into()),
        change,
        reason: "update memory lifecycle".into(),
        evidence_refs: vec!["message:lifecycle".into()],
        requested_by: actor(),
        idempotency_key: key.into(),
        schema_version: 1,
        policy_version: 1,
        origin_run_id: None,
        origin_node_instance_id: None,
    }
}

fn content(text: &str) -> LongTermMemoryContentV1 {
    LongTermMemoryContentV1 {
        schema_version: 1,
        text: text.into(),
        tags: vec!["story".into(), "lore".into(), "story".into()],
        attributes: BTreeMap::from([("source".into(), json!("narrator"))]),
    }
}

fn actor() -> ActorRef {
    ActorRef {
        kind: ActorKind::User,
        id: Some("user-1".into()),
    }
}

async fn approve(
    store: &crate::SqliteStore,
    proposal_id: &str,
    key: &str,
) -> zhuangsheng_core::memory::MemoryChangeProposalView {
    store
        .decide_memory_proposal(DecideMemoryProposalCommand {
            proposal_id: proposal_id.into(),
            expected_status: MemoryProposalStatus::AwaitingReview,
            decision: MemoryProposalDecision::Approve,
            actor: actor(),
            idempotency_key: key.into(),
        })
        .await
        .unwrap()
}

async fn apply(
    store: &crate::SqliteStore,
    proposal_id: &str,
    key: &str,
) -> zhuangsheng_core::memory::MemoryChangeProposalView {
    store
        .apply_memory_proposal(ApplyMemoryProposalCommand {
            proposal_id: proposal_id.into(),
            expected_status: MemoryProposalStatus::Approved,
            idempotency_key: key.into(),
        })
        .await
        .unwrap()
}

async fn count(store: &crate::SqliteStore, table: &str) -> i64 {
    store
        .db
        .query_one_raw(sql(
            &format!("SELECT COUNT(*) AS count FROM {table}"),
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap()
}
