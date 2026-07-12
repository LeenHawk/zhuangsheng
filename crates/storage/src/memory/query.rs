use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    memory::{
        LongTermMemoryContentV1, LongTermMemoryRecordView, LongTermMemoryStatus,
        MemoryChangeProposalView, MemoryProposalChangeType, MemoryProposalStatus,
    },
    state::{ActorKind, ActorRef},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

pub(crate) async fn load_proposal<C: ConnectionTrait>(
    connection: &C,
    proposal_id: &str,
) -> StorageResult<MemoryChangeProposalView> {
    let row = connection.query_one_raw(sql(
        "SELECT id, scope_id, memory_id, expected_head_commit_id, change_type, content_object_id, reason, evidence_refs_json, requested_by_kind, requested_by_id, schema_version, policy_version, origin_run_id, origin_node_instance_id, applied_commit_id, status, created_at, updated_at FROM memory_change_proposals WHERE id = ?",
        vec![proposal_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound {
        kind: "memory_proposal",
        id: proposal_id.into(),
    })?;
    let evidence: String = row.try_get("", "evidence_refs_json")?;
    Ok(MemoryChangeProposalView {
        id: row.try_get("", "id")?,
        scope_id: row.try_get("", "scope_id")?,
        memory_id: row.try_get("", "memory_id")?,
        expected_head_commit_id: row.try_get("", "expected_head_commit_id")?,
        change_type: parse_change(&row.try_get::<String>("", "change_type")?)?,
        content_ref: row.try_get("", "content_object_id")?,
        reason: row.try_get("", "reason")?,
        evidence_refs: serde_json::from_str(&evidence)
            .map_err(|error| StorageError::Integrity(error.to_string()))?,
        requested_by: ActorRef {
            kind: parse_actor(&row.try_get::<String>("", "requested_by_kind")?)?,
            id: row.try_get("", "requested_by_id")?,
        },
        schema_version: to_u32(row.try_get("", "schema_version")?)?,
        policy_version: to_u32(row.try_get("", "policy_version")?)?,
        origin_run_id: row.try_get("", "origin_run_id")?,
        origin_node_instance_id: row.try_get("", "origin_node_instance_id")?,
        applied_commit_id: row.try_get("", "applied_commit_id")?,
        status: parse_proposal_status(&row.try_get::<String>("", "status")?)?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub(crate) async fn load_record<C: ConnectionTrait>(
    connection: &C,
    memory_id: &str,
) -> StorageResult<LongTermMemoryRecordView> {
    let row = connection.query_one_raw(sql(
        "SELECT id, scope_id, status, head_commit_id, current_content_object_id, created_at, updated_at FROM memory_records WHERE id = ?",
        vec![memory_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound {
        kind: "memory_record",
        id: memory_id.into(),
    })?;
    let content_ref: Option<String> = row.try_get("", "current_content_object_id")?;
    let content = match &content_ref {
        Some(object_id) => Some(load_object_json(connection, object_id).await?),
        None => None,
    };
    Ok(LongTermMemoryRecordView {
        id: row.try_get("", "id")?,
        scope_id: row.try_get("", "scope_id")?,
        status: parse_record_status(&row.try_get::<String>("", "status")?)?,
        head_commit_id: row.try_get("", "head_commit_id")?,
        content_ref,
        content,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub(crate) fn parse_proposal_status(value: &str) -> StorageResult<MemoryProposalStatus> {
    Ok(match value {
        "proposed" => MemoryProposalStatus::Proposed,
        "awaiting_confirmation" => MemoryProposalStatus::AwaitingConfirmation,
        "awaiting_review" => MemoryProposalStatus::AwaitingReview,
        "approved" => MemoryProposalStatus::Approved,
        "rejected" => MemoryProposalStatus::Rejected,
        "applied" => MemoryProposalStatus::Applied,
        "conflicted" => MemoryProposalStatus::Conflicted,
        _ => {
            return Err(StorageError::Integrity(
                "invalid memory proposal status".into(),
            ));
        }
    })
}

pub(crate) fn proposal_status(value: MemoryProposalStatus) -> &'static str {
    match value {
        MemoryProposalStatus::Proposed => "proposed",
        MemoryProposalStatus::AwaitingConfirmation => "awaiting_confirmation",
        MemoryProposalStatus::AwaitingReview => "awaiting_review",
        MemoryProposalStatus::Approved => "approved",
        MemoryProposalStatus::Rejected => "rejected",
        MemoryProposalStatus::Applied => "applied",
        MemoryProposalStatus::Conflicted => "conflicted",
    }
}

pub(crate) fn actor_kind(value: ActorKind) -> &'static str {
    match value {
        ActorKind::User => "user",
        ActorKind::System => "system",
        ActorKind::Node => "node",
        ActorKind::Tool => "tool",
        ActorKind::Application => "application",
    }
}

fn parse_change(value: &str) -> StorageResult<MemoryProposalChangeType> {
    Ok(match value {
        "create" => MemoryProposalChangeType::Create,
        "replace_content" => MemoryProposalChangeType::ReplaceContent,
        "mark_obsolete" => MemoryProposalChangeType::MarkObsolete,
        "delete_tombstone" => MemoryProposalChangeType::DeleteTombstone,
        _ => {
            return Err(StorageError::Integrity(
                "invalid memory proposal change".into(),
            ));
        }
    })
}

fn parse_record_status(value: &str) -> StorageResult<LongTermMemoryStatus> {
    Ok(match value {
        "reserved" => LongTermMemoryStatus::Reserved,
        "active" => LongTermMemoryStatus::Active,
        "obsolete" => LongTermMemoryStatus::Obsolete,
        "deleted" => LongTermMemoryStatus::Deleted,
        "discarded" => LongTermMemoryStatus::Discarded,
        _ => {
            return Err(StorageError::Integrity(
                "invalid memory record status".into(),
            ));
        }
    })
}

fn parse_actor(value: &str) -> StorageResult<ActorKind> {
    Ok(match value {
        "user" => ActorKind::User,
        "system" => ActorKind::System,
        "node" => ActorKind::Node,
        "tool" => ActorKind::Tool,
        "application" => ActorKind::Application,
        _ => return Err(StorageError::Integrity("invalid memory actor kind".into())),
    })
}

fn to_u32(value: i64) -> StorageResult<u32> {
    u32::try_from(value).map_err(|_| StorageError::Integrity("invalid memory version".into()))
}

#[allow(dead_code)]
fn _content_type(_: LongTermMemoryContentV1) {}
