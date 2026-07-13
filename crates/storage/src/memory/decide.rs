use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    application::memory::{DecideMemoryProposalCommand, MemoryProposalDecision},
    canonical,
    memory::{MemoryChangeProposalView, MemoryProposalChangeType, MemoryProposalStatus},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, now_ms, sql},
};

use super::{
    query::{actor_kind, load_proposal, proposal_status},
    receipt,
    run_events::append_proposal_run_event,
};

impl SqliteStore {
    pub async fn decide_memory_proposal(
        &self,
        command: DecideMemoryProposalCommand,
    ) -> StorageResult<MemoryChangeProposalView> {
        if command.idempotency_key.is_empty()
            || !matches!(
                command.expected_status,
                MemoryProposalStatus::AwaitingConfirmation | MemoryProposalStatus::AwaitingReview
            )
        {
            return Err(StorageError::InvalidArgument(
                "memory proposal decision precondition is invalid".into(),
            ));
        }
        let digest = canonical::hash(&command)?;
        let receipt_scope = format!("memory-proposal:{}:decisions", command.proposal_id);
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
        if transaction.query_one_raw(sql(
            "SELECT 1 AS present FROM wait_blockers WHERE blocker_kind='memory_proposal' AND blocker_id=? AND status='open'",
            vec![command.proposal_id.clone().into()],
        )).await?.is_some() {
            return Err(StorageError::Conflict("memory_proposal_wait_required"));
        }
        let view = decide_in(&transaction, &command, now).await?;
        receipt::finish(
            &transaction,
            &receipt_scope,
            &command.idempotency_key,
            &digest,
            "decide_memory_proposal",
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

pub(crate) async fn decide_in<C: ConnectionTrait>(
    connection: &C,
    command: &DecideMemoryProposalCommand,
    now: i64,
) -> StorageResult<MemoryChangeProposalView> {
    if command.idempotency_key.is_empty()
        || !matches!(
            command.expected_status,
            MemoryProposalStatus::AwaitingConfirmation | MemoryProposalStatus::AwaitingReview
        )
    {
        return Err(StorageError::InvalidArgument(
            "memory proposal decision precondition is invalid".into(),
        ));
    }
    let proposal = load_proposal(connection, &command.proposal_id).await?;
    if proposal.status != command.expected_status {
        return Err(StorageError::Conflict("memory_proposal_status"));
    }
    let next = match command.decision {
        MemoryProposalDecision::Approve => MemoryProposalStatus::Approved,
        MemoryProposalDecision::Reject => MemoryProposalStatus::Rejected,
    };
    let updated = connection.execute_raw(sql(
            "UPDATE memory_change_proposals SET status = ?, updated_at = ? WHERE id = ? AND status = ?",
            vec![proposal_status(next).into(), now.into(), command.proposal_id.clone().into(), proposal_status(command.expected_status).into()],
        )).await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("memory_proposal_status"));
    }
    insert_transition(connection, command, next, now).await?;
    if next == MemoryProposalStatus::Rejected
        && proposal.change_type == MemoryProposalChangeType::Create
    {
        let discarded = connection.execute_raw(sql(
                "UPDATE memory_records SET status = 'discarded', updated_at = ? WHERE id = ? AND status = 'reserved' AND head_commit_id IS NULL",
                vec![now.into(), proposal.memory_id.clone().into()],
            )).await?;
        if discarded.rows_affected() != 1 {
            return Err(StorageError::Conflict("memory_reservation"));
        }
    }
    let view = load_proposal(connection, &command.proposal_id).await?;
    append_proposal_run_event(
        connection,
        &view,
        "memory.proposal.status_changed",
        proposal_status(next),
        now,
    )
    .await?;
    Ok(view)
}

async fn insert_transition<C: ConnectionTrait>(
    connection: &C,
    command: &DecideMemoryProposalCommand,
    next: MemoryProposalStatus,
    now: i64,
) -> StorageResult<()> {
    let row = connection.query_one_raw(sql(
        "SELECT COALESCE(MAX(transition_no), 0) + 1 AS next_no FROM memory_proposal_transitions WHERE proposal_id = ?",
        vec![command.proposal_id.clone().into()],
    )).await?.expect("transition aggregate returns a row");
    let number: i64 = row.try_get("", "next_no")?;
    connection.execute_raw(sql(
        "INSERT INTO memory_proposal_transitions (id, proposal_id, transition_no, from_status, to_status, actor_kind, actor_id, command_idempotency_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![new_id("memtransition").into(), command.proposal_id.clone().into(), number.into(), proposal_status(command.expected_status).into(), proposal_status(next).into(), actor_kind(command.actor.kind).into(), command.actor.id.clone().into(), command.idempotency_key.clone().into(), now.into()],
    )).await?;
    Ok(())
}
