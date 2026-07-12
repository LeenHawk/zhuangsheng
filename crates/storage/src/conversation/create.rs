use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::CreateConversationCommand,
    canonical,
    conversation::{ConversationContextV1, ConversationRunProfile, ConversationView},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{
    contract::validate_run_spec,
    events::append_event,
    read::load_conversation,
    receipt::{Receipt, finish, replay},
};

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
            "defaultRun":command.default_run,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(view) = replay::<_, ConversationView>(
            &transaction,
            "conversation:create",
            &command.idempotency_key,
            &digest,
        )
        .await?
        {
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
        if let Some(run) = &command.default_run {
            validate_run_spec(&transaction, run).await?;
        }
        let run_profile = command
            .default_run
            .clone()
            .map(|run| ConversationRunProfile {
                run,
                revision_no: 1,
            });
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
            "INSERT INTO conversations (id, context_id, active_branch_id, active_head_commit_id, default_graph_revision_id, default_reply_output_key, default_input_shape, run_profile_revision_no, title, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![conversation_id.clone().into(), context_id.clone().into(), branch_id.clone().into(), commit_id.clone().into(), run_profile.as_ref().map(|profile| profile.run.graph_revision_id.clone()).into(), run_profile.as_ref().map(|profile| profile.run.reply_output_key.clone()).into(), run_profile.as_ref().map(|_| "conversation_message_v1".to_owned()).into(), run_profile.as_ref().map(|profile| profile.revision_no as i64).into(), command.title.clone().into(), now.into(), now.into()],
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
            "conversation.created",
            &json!({"schemaVersion":1,"conversationId":conversation_id,"contextId":context_id,"branchId":branch_id,"headCommitId":commit_id,"runProfile":run_profile}),
            now,
        )
        .await?;
        let view = ConversationView {
            id: conversation_id,
            title: command.title,
            context_id,
            active_branch_id: branch_id,
            active_head_commit_id: commit_id,
            run_profile,
            created_at: now,
            updated_at: now,
        };
        finish(
            &transaction,
            Receipt {
                scope: "conversation:create",
                key: &command.idempotency_key,
                digest: &digest,
                command_kind: "conversation.create",
                resource_kind: "conversation",
                resource_id: &view.id,
                now,
            },
            &view,
        )
        .await?;
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

async fn add_ref<C: sea_orm::ConnectionTrait>(
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
