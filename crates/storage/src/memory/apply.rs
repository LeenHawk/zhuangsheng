use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::memory::ApplyMemoryProposalCommand,
    canonical,
    memory::{
        LongTermMemoryStatus, MemoryChangeProposalView, MemoryProposalChangeType,
        MemoryProposalStatus,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, now_ms, put_inline_object, sql},
};

use super::{
    query::{actor_kind, load_proposal, load_record},
    receipt,
};

impl SqliteStore {
    pub async fn apply_memory_proposal(
        &self,
        command: ApplyMemoryProposalCommand,
    ) -> StorageResult<MemoryChangeProposalView> {
        if command.idempotency_key.is_empty()
            || command.expected_status != MemoryProposalStatus::Approved
        {
            return Err(StorageError::InvalidArgument(
                "memory apply precondition is invalid".into(),
            ));
        }
        let digest = canonical::hash(&command)?;
        let receipt_scope = format!("memory-proposal:{}:apply", command.proposal_id);
        let now = now_ms();
        let transaction = self.db.begin().await?;
        if let Some(view) = receipt::replay(
            &transaction,
            &receipt_scope,
            &command.idempotency_key,
            &digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(view);
        }
        let proposal = load_proposal(&transaction, &command.proposal_id).await?;
        if proposal.status != command.expected_status {
            return Err(StorageError::Conflict("memory_proposal_status"));
        }
        let record = load_record(&transaction, &proposal.memory_id).await?;
        if record.scope_id != proposal.scope_id
            || record.head_commit_id != proposal.expected_head_commit_id
        {
            let view = mark_conflicted(
                &transaction,
                &command,
                &proposal,
                &receipt_scope,
                &digest,
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(view);
        }
        validate_transition(&proposal, record.status)?;
        let next_status = next_status(proposal.change_type);
        let content_ref = match proposal.change_type {
            MemoryProposalChangeType::Create | MemoryProposalChangeType::ReplaceContent => {
                proposal.content_ref.clone()
            }
            MemoryProposalChangeType::MarkObsolete => record.content_ref.clone(),
            MemoryProposalChangeType::DeleteTombstone => None,
        };
        let commit_id = new_id("commit");
        let sequence = match &record.head_commit_id {
            Some(head) => load_sequence(&transaction, head).await? + 1,
            None => 1,
        };
        let version_object = version_payload(&proposal, &content_ref);
        let version_object_id =
            put_inline_object(&transaction, &canonical::to_vec(&version_object)?, now).await?;
        insert_commit(
            &transaction,
            &proposal,
            &commit_id,
            record.head_commit_id.as_deref(),
            &version_object_id,
            sequence,
            now,
        )
        .await?;
        update_record(
            &transaction,
            &proposal,
            &record,
            &commit_id,
            next_status,
            content_ref.as_deref(),
            now,
        )
        .await?;
        finalize_proposal(&transaction, &command, &commit_id, now).await?;
        update_scope_and_search(
            &transaction,
            &proposal,
            next_status,
            content_ref.as_deref(),
            now,
        )
        .await?;
        add_refs(
            &transaction,
            &proposal,
            &commit_id,
            &version_object_id,
            content_ref.as_deref(),
            now,
        )
        .await?;
        append_event(&transaction, &proposal, &commit_id, sequence, now).await?;
        let view = load_proposal(&transaction, &command.proposal_id).await?;
        receipt::finish(
            &transaction,
            &receipt_scope,
            &command.idempotency_key,
            &digest,
            "apply_memory_proposal",
            "memory_proposal",
            &command.proposal_id,
            &view,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(view)
    }
}

fn validate_transition(
    proposal: &MemoryChangeProposalView,
    status: LongTermMemoryStatus,
) -> StorageResult<()> {
    let valid = match proposal.change_type {
        MemoryProposalChangeType::Create => status == LongTermMemoryStatus::Reserved,
        MemoryProposalChangeType::ReplaceContent => status == LongTermMemoryStatus::Active,
        MemoryProposalChangeType::MarkObsolete => status == LongTermMemoryStatus::Active,
        MemoryProposalChangeType::DeleteTombstone => {
            matches!(
                status,
                LongTermMemoryStatus::Active | LongTermMemoryStatus::Obsolete
            )
        }
    };
    if valid {
        Ok(())
    } else {
        Err(StorageError::Conflict("memory_record_status"))
    }
}

fn next_status(change: MemoryProposalChangeType) -> LongTermMemoryStatus {
    match change {
        MemoryProposalChangeType::Create | MemoryProposalChangeType::ReplaceContent => {
            LongTermMemoryStatus::Active
        }
        MemoryProposalChangeType::MarkObsolete => LongTermMemoryStatus::Obsolete,
        MemoryProposalChangeType::DeleteTombstone => LongTermMemoryStatus::Deleted,
    }
}

fn record_status(status: LongTermMemoryStatus) -> &'static str {
    match status {
        LongTermMemoryStatus::Reserved => "reserved",
        LongTermMemoryStatus::Active => "active",
        LongTermMemoryStatus::Obsolete => "obsolete",
        LongTermMemoryStatus::Deleted => "deleted",
        LongTermMemoryStatus::Discarded => "discarded",
    }
}

fn version_payload(proposal: &MemoryChangeProposalView, content_ref: &Option<String>) -> Value {
    json!({
        "schemaVersion":1,
        "changeType":proposal.change_type,
        "status":record_status(next_status(proposal.change_type)),
        "contentRef":content_ref,
    })
}

async fn load_sequence<C: ConnectionTrait>(connection: &C, head: &str) -> StorageResult<i64> {
    let row = connection.query_one(sql(
        "SELECT sequence_no FROM version_commits WHERE id = ? AND aggregate_kind = 'long_term_memory'",
        vec![head.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("memory head commit missing".into()))?;
    row.try_get("", "sequence_no").map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
async fn insert_commit<C: ConnectionTrait>(
    connection: &C,
    proposal: &MemoryChangeProposalView,
    commit_id: &str,
    parent: Option<&str>,
    version_object_id: &str,
    sequence: i64,
    now: i64,
) -> StorageResult<()> {
    let snapshot: Option<String> = (parent.is_none()).then(|| version_object_id.into());
    let patch: Option<String> = parent.is_some().then(|| version_object_id.into());
    connection.execute(sql(
        "INSERT INTO version_commits (id, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, patch_object_id, initial_snapshot_object_id, schema_version, policy_version, author_kind, author_id, origin_run_id, origin_node_instance_id, source_proposal_id, created_at) VALUES (?, 'long_term_memory', ?, 'global', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![commit_id.into(), proposal.memory_id.clone().into(), sequence.into(), format!("memory-proposal:{}", proposal.id).into(), patch.into(), snapshot.into(), i64::from(proposal.schema_version).into(), i64::from(proposal.policy_version).into(), actor_kind(proposal.requested_by.kind).into(), proposal.requested_by.id.clone().into(), proposal.origin_run_id.clone().into(), proposal.origin_node_instance_id.clone().into(), proposal.id.clone().into(), now.into()],
    )).await?;
    if let Some(parent) = parent {
        connection.execute(sql(
            "INSERT INTO commit_parents (commit_id, parent_commit_id, parent_order) VALUES (?, ?, 0)",
            vec![commit_id.into(), parent.into()],
        )).await?;
    }
    Ok(())
}

async fn update_record<C: ConnectionTrait>(
    connection: &C,
    proposal: &MemoryChangeProposalView,
    record: &zhuangsheng_core::memory::LongTermMemoryRecordView,
    commit_id: &str,
    status: LongTermMemoryStatus,
    content_ref: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    let updated = connection.execute(sql(
        "UPDATE memory_records SET status = ?, head_commit_id = ?, current_content_object_id = ?, updated_at = ? WHERE id = ? AND status = ? AND ((head_commit_id IS NULL AND ? IS NULL) OR head_commit_id = ?)",
        vec![record_status(status).into(), commit_id.into(), content_ref.map(String::from).into(), now.into(), proposal.memory_id.clone().into(), record_status(record.status).into(), record.head_commit_id.clone().into(), record.head_commit_id.clone().into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("memory_head"));
    }
    Ok(())
}

async fn finalize_proposal<C: ConnectionTrait>(
    connection: &C,
    command: &ApplyMemoryProposalCommand,
    commit_id: &str,
    now: i64,
) -> StorageResult<()> {
    let updated = connection.execute(sql(
        "UPDATE memory_change_proposals SET status = 'applied', applied_commit_id = ?, updated_at = ? WHERE id = ? AND status = 'approved'",
        vec![commit_id.into(), now.into(), command.proposal_id.clone().into()],
    )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("memory_proposal_status"));
    }
    insert_transition(
        connection,
        &command.proposal_id,
        "approved",
        "applied",
        &command.idempotency_key,
        now,
    )
    .await
}

async fn update_scope_and_search<C: ConnectionTrait>(
    connection: &C,
    proposal: &MemoryChangeProposalView,
    status: LongTermMemoryStatus,
    content_ref: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    connection
        .execute(sql(
            "UPDATE memory_scopes SET revision_no = revision_no + 1, updated_at = ? WHERE id = ?",
            vec![now.into(), proposal.scope_id.clone().into()],
        ))
        .await?;
    connection
        .execute(sql(
            "DELETE FROM memory_search WHERE memory_id = ?",
            vec![proposal.memory_id.clone().into()],
        ))
        .await?;
    if matches!(
        status,
        LongTermMemoryStatus::Active | LongTermMemoryStatus::Obsolete
    ) {
        let content_id = content_ref
            .ok_or_else(|| StorageError::Integrity("memory content ref missing".into()))?;
        let content: zhuangsheng_core::memory::LongTermMemoryContentV1 =
            crate::graph::helpers::load_object_json(connection, content_id).await?;
        connection
            .execute(sql(
                "INSERT INTO memory_search (memory_id, scope_id, text, tags) VALUES (?, ?, ?, ?)",
                vec![
                    proposal.memory_id.clone().into(),
                    proposal.scope_id.clone().into(),
                    content.text.into(),
                    content.tags.join(" ").into(),
                ],
            ))
            .await?;
    }
    Ok(())
}

async fn add_refs<C: ConnectionTrait>(
    connection: &C,
    proposal: &MemoryChangeProposalView,
    commit_id: &str,
    version_object_id: &str,
    content_ref: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    connection.execute(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'version_commit', ?, 'memory_version', ?)",
        vec![version_object_id.into(), commit_id.into(), now.into()],
    )).await?;
    connection.execute(sql(
        "DELETE FROM content_object_refs WHERE owner_kind = 'memory_record' AND owner_id = ? AND role = 'current_content'",
        vec![proposal.memory_id.clone().into()],
    )).await?;
    if let Some(content_ref) = content_ref {
        for (owner_kind, owner_id, role) in [
            (
                "memory_record",
                proposal.memory_id.as_str(),
                "current_content",
            ),
            ("version_commit", commit_id, "memory_content"),
        ] {
            connection.execute(sql(
                "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, ?, ?, ?, ?)",
                vec![content_ref.into(), owner_kind.into(), owner_id.into(), role.into(), now.into()],
            )).await?;
        }
    }
    Ok(())
}

async fn append_event<C: ConnectionTrait>(
    connection: &C,
    proposal: &MemoryChangeProposalView,
    commit_id: &str,
    sequence: i64,
    now: i64,
) -> StorageResult<()> {
    connection.execute(sql(
        "INSERT OR IGNORE INTO domain_event_counters (aggregate_kind, aggregate_id, lineage_key, next_seq) VALUES ('long_term_memory', ?, 'global', 1)",
        vec![proposal.memory_id.clone().into()],
    )).await?;
    let row = connection.query_one(sql(
        "SELECT next_seq FROM domain_event_counters WHERE aggregate_kind = 'long_term_memory' AND aggregate_id = ? AND lineage_key = 'global'",
        vec![proposal.memory_id.clone().into()],
    )).await?.expect("memory event counter exists");
    let event_seq: i64 = row.try_get("", "next_seq")?;
    connection.execute(sql(
        "UPDATE domain_event_counters SET next_seq = next_seq + 1 WHERE aggregate_kind = 'long_term_memory' AND aggregate_id = ? AND lineage_key = 'global' AND next_seq = ?",
        vec![proposal.memory_id.clone().into(), event_seq.into()],
    )).await?;
    connection.execute(sql(
        "INSERT INTO domain_events (id, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json, created_at) VALUES (?, 'long_term_memory', ?, 'global', ?, 'memory.commit.applied', 1, ?, ?)",
        vec![new_id("domain_event").into(), proposal.memory_id.clone().into(), event_seq.into(), canonical::to_string(&json!({"schemaVersion":1,"proposalId":proposal.id,"commitId":commit_id,"sequenceNo":sequence}))?.into(), now.into()],
    )).await?;
    Ok(())
}

async fn mark_conflicted<C: ConnectionTrait>(
    connection: &C,
    command: &ApplyMemoryProposalCommand,
    proposal: &MemoryChangeProposalView,
    receipt_scope: &str,
    digest: &str,
    now: i64,
) -> StorageResult<MemoryChangeProposalView> {
    connection.execute(sql(
        "UPDATE memory_change_proposals SET status = 'conflicted', updated_at = ? WHERE id = ? AND status = 'approved'",
        vec![now.into(), command.proposal_id.clone().into()],
    )).await?;
    insert_transition(
        connection,
        &command.proposal_id,
        "approved",
        "conflicted",
        &command.idempotency_key,
        now,
    )
    .await?;
    let view = load_proposal(connection, &command.proposal_id).await?;
    receipt::finish(
        connection,
        receipt_scope,
        &command.idempotency_key,
        digest,
        "apply_memory_proposal",
        "memory_proposal",
        &proposal.id,
        &view,
        now,
    )
    .await?;
    Ok(view)
}

async fn insert_transition<C: ConnectionTrait>(
    connection: &C,
    proposal_id: &str,
    from: &str,
    to: &str,
    key: &str,
    now: i64,
) -> StorageResult<()> {
    let row = connection.query_one(sql(
        "SELECT COALESCE(MAX(transition_no), 0) + 1 AS next_no FROM memory_proposal_transitions WHERE proposal_id = ?",
        vec![proposal_id.into()],
    )).await?.expect("transition aggregate returns a row");
    let number: i64 = row.try_get("", "next_no")?;
    connection.execute(sql(
        "INSERT INTO memory_proposal_transitions (id, proposal_id, transition_no, from_status, to_status, actor_kind, command_idempotency_key, created_at) VALUES (?, ?, ?, ?, ?, 'application', ?, ?)",
        vec![new_id("memtransition").into(), proposal_id.into(), number.into(), from.into(), to.into(), key.into(), now.into()],
    )).await?;
    Ok(())
}
