use serde_json::json;
use zhuangsheng_core::{
    application::{
        context::CommitContextPatchCommand, conversation::SubmitConversationTurnCommand,
    },
    canonical,
    conversation::{
        ConversationContextMessageV1, ConversationContextV1, ConversationMessageRole,
        ConversationMessageSource, ConversationView,
    },
    llm::ir::validate_content_parts,
    state::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch},
};

use crate::{StorageError, StorageResult};

pub(super) struct PreparedSubmission {
    pub content_bytes: Vec<u8>,
    pub scope: String,
    pub digest: String,
}

pub(super) fn prepare(
    command: &SubmitConversationTurnCommand,
) -> StorageResult<PreparedSubmission> {
    if command.conversation_id.is_empty()
        || command.expected_head_commit_id.is_empty()
        || command.idempotency_key.is_empty()
        || command.idempotency_key.len() > 128
    {
        return Err(StorageError::InvalidArgument(
            "invalid submit turn command".into(),
        ));
    }
    validate_content_parts(&command.user_content, true)
        .map_err(|error| StorageError::InvalidArgument(error.to_string()))?;
    let content_bytes = canonical::to_vec(&command.user_content)?;
    let scope = format!("conversation:turns:{}", command.conversation_id);
    let digest = canonical::hash(&json!({
        "schemaVersion":1,"command":"submit_conversation_turn",
        "conversationId":command.conversation_id,
        "expectedHeadCommitId":command.expected_head_commit_id,
        "userContentHash":canonical::hash_bytes(&content_bytes),"run":command.run,
    }))?;
    Ok(PreparedSubmission {
        content_bytes,
        scope,
        digest,
    })
}

pub(super) fn user_message_patch(
    conversation: &ConversationView,
    command: &SubmitConversationTurnCommand,
    context: &ConversationContextV1,
    turn_id: &str,
    message_id: &str,
    content_id: &str,
) -> StorageResult<(ConversationContextMessageV1, CommitContextPatchCommand)> {
    let message = ConversationContextMessageV1 {
        message_id: message_id.into(),
        turn_id: turn_id.into(),
        role: ConversationMessageRole::User,
        source: ConversationMessageSource::UserInput,
        content_ref: content_id.into(),
        parent_message_id: context.messages.last().map(|item| item.message_id.clone()),
        origin_run_id: None,
    };
    let value = serde_json::to_value(&message)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    Ok((
        message,
        CommitContextPatchCommand {
            patch: StatePatch {
                aggregate_kind: AggregateKind::WorkingContext,
                aggregate_id: conversation.context_id.clone(),
                lineage_key: conversation.active_branch_id.clone(),
                base_commit_id: command.expected_head_commit_id.clone(),
                operation_id: format!("conversation-user-message:{message_id}"),
                ops: vec![JsonPatchOp::Append {
                    path: "/messages".into(),
                    element_id: message_id.into(),
                    value,
                }],
                schema_version: 1,
                policy_version: 1,
                author: ActorRef {
                    kind: ActorKind::User,
                    id: None,
                },
            },
            origin_run_id: None,
            origin_node_instance_id: None,
        },
    ))
}
