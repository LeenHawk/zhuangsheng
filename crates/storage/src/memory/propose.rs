use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    application::memory::ProposeMemoryChangeCommand,
    canonical,
    memory::{
        MemoryChangeProposalView, MemoryProposalChangeInput, normalize_content,
        validate_proposal_material,
    },
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, now_ms, put_inline_object, sql},
};

use super::{
    query::{actor_kind, load_proposal},
    receipt,
};

impl SqliteStore {
    pub async fn propose_memory_change(
        &self,
        mut command: ProposeMemoryChangeCommand,
    ) -> StorageResult<MemoryChangeProposalView> {
        if command.idempotency_key.is_empty() {
            return Err(StorageError::InvalidArgument(
                "memory proposal idempotency key is required".into(),
            ));
        }
        validate_proposal_material(
            &command.scope_id,
            &command.reason,
            &command.evidence_refs,
            command.schema_version,
            command.policy_version,
        )?;
        let (change_type, normalized_content) = match &mut command.change {
            MemoryProposalChangeInput::Create { content } => {
                let normalized = normalize_content(content.clone())?;
                *content = normalized.clone();
                ("create", Some(normalized))
            }
            MemoryProposalChangeInput::ReplaceContent { content } => {
                let normalized = normalize_content(content.clone())?;
                *content = normalized.clone();
                ("replace_content", Some(normalized))
            }
            MemoryProposalChangeInput::MarkObsolete => ("mark_obsolete", None),
            MemoryProposalChangeInput::DeleteTombstone => ("delete_tombstone", None),
        };
        validate_target_shape(&command, change_type)?;
        let digest = canonical::hash(&command)?;
        let receipt_scope = format!("memory-scope:{}:proposals", command.scope_id);
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
        transaction.execute(sql(
            "INSERT OR IGNORE INTO memory_scopes (id, revision_no, created_at, updated_at) VALUES (?, 0, ?, ?)",
            vec![command.scope_id.clone().into(), now.into(), now.into()],
        )).await?;
        let memory_id = match change_type {
            "create" => reserve_memory(&transaction, &command.scope_id, now).await?,
            _ => validate_existing_target(&transaction, &command).await?,
        };
        let content_id = match normalized_content {
            Some(content) => {
                Some(put_inline_object(&transaction, &canonical::to_vec(&content)?, now).await?)
            }
            None => None,
        };
        let proposal_id = new_id("memproposal");
        transaction.execute(sql(
            "INSERT INTO memory_change_proposals (id, scope_id, memory_id, expected_head_commit_id, change_type, content_object_id, reason, evidence_refs_json, requested_by_kind, requested_by_id, schema_version, policy_version, origin_run_id, origin_node_instance_id, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'awaiting_review', ?, ?)",
            vec![proposal_id.clone().into(), command.scope_id.clone().into(), memory_id.into(), command.expected_head_commit_id.clone().into(), change_type.into(), content_id.clone().into(), command.reason.clone().into(), canonical::to_string(&command.evidence_refs)?.into(), actor_kind(command.requested_by.kind).into(), command.requested_by.id.clone().into(), i64::from(command.schema_version).into(), i64::from(command.policy_version).into(), command.origin_run_id.clone().into(), command.origin_node_instance_id.clone().into(), now.into(), now.into()],
        )).await?;
        insert_initial_transitions(
            &transaction,
            &proposal_id,
            actor_kind(command.requested_by.kind),
            command.requested_by.id.as_deref(),
            now,
        )
        .await?;
        if let Some(content_id) = &content_id {
            transaction.execute(sql(
                "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'memory_proposal', ?, 'content', ?)",
                vec![content_id.clone().into(), proposal_id.clone().into(), now.into()],
            )).await?;
        }
        let view = load_proposal(&transaction, &proposal_id).await?;
        receipt::finish(
            &transaction,
            &receipt_scope,
            &command.idempotency_key,
            &digest,
            "propose_memory_change",
            "memory_proposal",
            &proposal_id,
            &view,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(view)
    }
}

fn validate_target_shape(
    command: &ProposeMemoryChangeCommand,
    change_type: &str,
) -> StorageResult<()> {
    let create = change_type == "create";
    if create != command.memory_id.is_none()
        || create != command.expected_head_commit_id.is_none()
        || command.origin_node_instance_id.is_some() && command.origin_run_id.is_none()
    {
        return Err(StorageError::InvalidArgument(
            "memory proposal target or origin is invalid".into(),
        ));
    }
    Ok(())
}

async fn reserve_memory<C: ConnectionTrait>(
    connection: &C,
    scope_id: &str,
    now: i64,
) -> StorageResult<String> {
    let memory_id = new_id("memory");
    connection.execute(sql(
        "INSERT INTO memory_records (id, scope_id, status, created_at, updated_at) VALUES (?, ?, 'reserved', ?, ?)",
        vec![memory_id.clone().into(), scope_id.into(), now.into(), now.into()],
    )).await?;
    Ok(memory_id)
}

async fn validate_existing_target<C: ConnectionTrait>(
    connection: &C,
    command: &ProposeMemoryChangeCommand,
) -> StorageResult<String> {
    let memory_id = command.memory_id.as_deref().expect("validated target");
    let row = connection
        .query_one(sql(
            "SELECT scope_id, head_commit_id, status FROM memory_records WHERE id = ?",
            vec![memory_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "memory_record",
            id: memory_id.into(),
        })?;
    let scope: String = row.try_get("", "scope_id")?;
    let head: Option<String> = row.try_get("", "head_commit_id")?;
    let status: String = row.try_get("", "status")?;
    if scope != command.scope_id
        || head != command.expected_head_commit_id
        || !matches!(status.as_str(), "active" | "obsolete")
    {
        return Err(StorageError::Conflict("memory_head"));
    }
    Ok(memory_id.into())
}

async fn insert_initial_transitions<C: ConnectionTrait>(
    connection: &C,
    proposal_id: &str,
    actor_kind: &str,
    actor_id: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    connection.execute(sql(
        "INSERT INTO memory_proposal_transitions (id, proposal_id, transition_no, to_status, actor_kind, actor_id, created_at) VALUES (?, ?, 1, 'proposed', ?, ?, ?)",
        vec![new_id("memtransition").into(), proposal_id.into(), actor_kind.into(), actor_id.map(String::from).into(), now.into()],
    )).await?;
    connection.execute(sql(
        "INSERT INTO memory_proposal_transitions (id, proposal_id, transition_no, from_status, to_status, actor_kind, actor_id, created_at) VALUES (?, ?, 2, 'proposed', 'awaiting_review', ?, ?, ?)",
        vec![new_id("memtransition").into(), proposal_id.into(), actor_kind.into(), actor_id.map(String::from).into(), now.into()],
    )).await?;
    Ok(())
}
