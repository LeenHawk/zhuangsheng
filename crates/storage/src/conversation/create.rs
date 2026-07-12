use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::CreateConversationCommand,
    canonical,
    conversation::{ConversationContextV1, ConversationView},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

use super::read::load_conversation;

impl SqliteStore {
    pub async fn create_conversation_at(
        &self,
        command: CreateConversationCommand,
        now: i64,
    ) -> StorageResult<ConversationView> {
        validate(&command)?;
        let digest = canonical::hash(&json!({
            "schemaVersion":1,
            "command":"create_conversation",
            "title":command.title,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(view) = replay(&transaction, &command.idempotency_key, &digest).await? {
            let current = load_conversation(&transaction, &view.id).await?;
            if current.context_id != view.context_id {
                return Err(StorageError::Integrity(
                    "conversation receipt identity is corrupt".into(),
                ));
            }
            transaction.commit().await?;
            return Ok(view);
        }
        let conversation_id = new_id("conversation");
        let context_id = new_id("context");
        let branch_id = new_id("branch");
        let commit_id = new_id("commit");
        let snapshot = ConversationContextV1::empty();
        let snapshot_id =
            put_inline_object(&transaction, &canonical::to_vec(&snapshot)?, now).await?;
        transaction.execute_raw(sql(
            "INSERT INTO contexts (id, kind, status, created_at, updated_at) VALUES (?, 'conversation', 'active', ?, ?)",
            vec![context_id.clone().into(), now.into(), now.into()],
        )).await?;
        transaction.execute_raw(sql(
            "INSERT INTO version_commits (id, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, initial_snapshot_object_id, schema_version, policy_version, author_kind, created_at) VALUES (?, 'working_context', ?, ?, 1, ?, ?, 1, 1, 'application', ?)",
            vec![commit_id.clone().into(), context_id.clone().into(), branch_id.clone().into(), format!("conversation-root:{conversation_id}").into(), snapshot_id.clone().into(), now.into()],
        )).await?;
        transaction.execute_raw(sql(
            "INSERT INTO context_branches (id, context_id, fork_commit_id, head_commit_id, creation_operation_id, status, pinned, audit_hold, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 'active', 0, 0, ?, ?)",
            vec![branch_id.clone().into(), context_id.clone().into(), commit_id.clone().into(), commit_id.clone().into(), format!("conversation-root-branch:{conversation_id}").into(), now.into(), now.into()],
        )).await?;
        transaction.execute_raw(sql(
            "INSERT INTO materialized_projections (aggregate_kind, aggregate_id, lineage_key, head_commit_id, projection_json, schema_version, updated_at) VALUES ('working_context', ?, ?, ?, ?, 1, ?)",
            vec![context_id.clone().into(), branch_id.clone().into(), commit_id.clone().into(), canonical::to_string(&snapshot)?.into(), now.into()],
        )).await?;
        transaction.execute_raw(sql(
            "INSERT INTO conversations (id, context_id, active_branch_id, active_head_commit_id, title, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![conversation_id.clone().into(), context_id.clone().into(), branch_id.clone().into(), commit_id.clone().into(), command.title.clone().into(), now.into(), now.into()],
        )).await?;
        add_ref(
            &transaction,
            &snapshot_id,
            "version_commit",
            &commit_id,
            "initial_snapshot",
            now,
        )
        .await?;
        append_event(
            &transaction,
            &conversation_id,
            &context_id,
            &branch_id,
            &commit_id,
            now,
        )
        .await?;
        let view = ConversationView {
            id: conversation_id,
            title: command.title,
            context_id,
            active_branch_id: branch_id,
            active_head_commit_id: commit_id,
            created_at: now,
            updated_at: now,
        };
        finish_receipt(&transaction, &command.idempotency_key, &digest, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }
}

fn validate(command: &CreateConversationCommand) -> StorageResult<()> {
    if command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
        || command.title.as_ref().is_some_and(|title| {
            title.is_empty() || title.len() > 200 || title.chars().any(char::is_control)
        })
    {
        return Err(StorageError::InvalidArgument(
            "invalid create conversation command".into(),
        ));
    }
    Ok(())
}

async fn replay<C: ConnectionTrait>(
    connection: &C,
    key: &str,
    digest: &str,
) -> StorageResult<Option<ConversationView>> {
    let row = connection.query_one_raw(sql("SELECT request_digest, result_object_id FROM application_command_receipts WHERE scope = 'conversation:create' AND idempotency_key = ? AND status = 'completed'", vec![key.into()])).await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    load_object_json(connection, &row.try_get::<String>("", "result_object_id")?)
        .await
        .map(Some)
}

async fn finish_receipt<C: ConnectionTrait>(
    connection: &C,
    key: &str,
    digest: &str,
    view: &ConversationView,
    now: i64,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(view)?, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, result_object_id, created_at, completed_at) VALUES ('conversation:create', ?, ?, 'conversation.create', 'conversation', ?, 'completed', ?, ?, ?)",
        vec![key.into(), digest.into(), view.id.clone().into(), object_id.clone().into(), now.into(), now.into()],
    )).await?;
    add_ref(
        connection,
        &object_id,
        "application_receipt",
        &format!("conversation:create:{key}"),
        "result",
        now,
    )
    .await
}

async fn append_event<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    context_id: &str,
    branch_id: &str,
    commit_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO domain_event_counters (aggregate_kind, aggregate_id, lineage_key, next_seq) VALUES ('conversation', ?, 'global', 2)",
        vec![conversation_id.into()],
    )).await?;
    connection.execute_raw(sql(
        "INSERT INTO domain_events (id, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json, created_at) VALUES (?, 'conversation', ?, 'global', 1, 'conversation.created', 1, ?, ?)",
        vec![new_id("domain_event").into(), conversation_id.into(), canonical::to_string(&json!({"schemaVersion":1,"conversationId":conversation_id,"contextId":context_id,"branchId":branch_id,"headCommitId":commit_id}))?.into(), now.into()],
    )).await?;
    Ok(())
}

async fn add_ref<C: ConnectionTrait>(
    connection: &C,
    object_id: &str,
    owner_kind: &str,
    owner_id: &str,
    role: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![object_id.into(), owner_kind.into(), owner_id.into(), role.into(), now.into()],
    )).await?;
    Ok(())
}
