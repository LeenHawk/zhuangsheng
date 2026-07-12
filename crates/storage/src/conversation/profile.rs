use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::conversation::UpdateConversationRunProfileCommand, canonical,
    conversation::ConversationRunProfile,
};

use crate::{SqliteStore, StorageError, StorageResult};

use super::{
    contract::validate_run_spec,
    events::append_event,
    read::load_conversation,
    receipt::{Receipt, finish, replay},
};

impl SqliteStore {
    pub async fn update_conversation_run_profile_at(
        &self,
        command: UpdateConversationRunProfileCommand,
        now: i64,
    ) -> StorageResult<ConversationRunProfile> {
        validate(&command)?;
        let scope = format!("conversation:run-profile:{}", command.conversation_id);
        let digest = canonical::hash(&json!({
            "schemaVersion":1,
            "command":"update_conversation_run_profile",
            "conversationId":command.conversation_id,
            "expectedRevisionNo":command.expected_revision_no,
            "run":command.run,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(profile) = replay::<_, ConversationRunProfile>(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
        )
        .await?
        {
            load_conversation(&transaction, &command.conversation_id).await?;
            transaction.commit().await?;
            return Ok(profile);
        }
        let current = load_conversation(&transaction, &command.conversation_id).await?;
        let current_revision = current
            .run_profile
            .as_ref()
            .map_or(0, |profile| profile.revision_no);
        if current_revision != command.expected_revision_no {
            return Err(StorageError::Conflict(
                "conversation_run_profile_revision_conflict",
            ));
        }
        validate_run_spec(&transaction, &command.run).await?;
        let revision_no = current_revision
            .checked_add(1)
            .ok_or_else(|| StorageError::Integrity("run profile revision overflow".into()))?;
        let revision_db = i64::try_from(revision_no)
            .map_err(|_| StorageError::Integrity("run profile revision overflow".into()))?;
        let result = transaction
            .execute_raw(Statement::from_sql_and_values(
                transaction.get_database_backend(),
                "UPDATE conversations SET default_graph_revision_id = ?, default_reply_output_key = ?, default_input_shape = 'conversation_message_v1', run_profile_revision_no = ?, updated_at = ? WHERE id = ? AND ((run_profile_revision_no IS NULL AND ? = 0) OR run_profile_revision_no = ?)",
                vec![command.run.graph_revision_id.clone().into(), command.run.reply_output_key.clone().into(), revision_db.into(), now.into(), command.conversation_id.clone().into(), (current_revision as i64).into(), (current_revision as i64).into()],
            ))
            .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::Conflict(
                "conversation_run_profile_revision_conflict",
            ));
        }
        let profile = ConversationRunProfile {
            run: command.run,
            revision_no,
        };
        append_event(
            &transaction,
            &command.conversation_id,
            "conversation.run_profile_updated",
            &json!({"schemaVersion":1,"conversationId":command.conversation_id,"runProfile":profile}),
            now,
        )
        .await?;
        finish(
            &transaction,
            Receipt {
                scope: &scope,
                key: &command.idempotency_key,
                digest: &digest,
                command_kind: "conversation.run_profile.update",
                resource_kind: "conversation",
                resource_id: &command.conversation_id,
                now,
            },
            &profile,
        )
        .await?;
        transaction.commit().await?;
        Ok(profile)
    }
}

fn validate(command: &UpdateConversationRunProfileCommand) -> StorageResult<()> {
    if command.conversation_id.is_empty()
        || command.conversation_id.len() > 128
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid update conversation run profile command".into(),
        ));
    }
    Ok(())
}
