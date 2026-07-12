use std::collections::{HashMap, HashSet};

use sea_orm::ConnectionTrait;
use zhuangsheng_core::conversation::{
    ConversationCandidateView, ConversationMessageRole, ConversationMessageView,
    ConversationTurnDetailView, ConversationTurnView, TurnCandidateStatus,
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

use super::read_candidate_error::load_error;
use super::read_candidate_validation::validate_candidate;

struct TurnAccumulator {
    turn: ConversationTurnView,
    selected_run_id: Option<String>,
    candidates: Vec<ConversationCandidateView>,
}

pub(super) async fn load_active_turns<C: ConnectionTrait>(
    connection: &C,
    conversation_id: &str,
    messages: &[ConversationMessageView],
) -> StorageResult<Vec<ConversationTurnDetailView>> {
    let mut active_order = Vec::new();
    let mut active = HashSet::new();
    for message in messages
        .iter()
        .filter(|message| message.role == ConversationMessageRole::User)
    {
        if !active.insert(message.turn_id.clone()) {
            return Err(StorageError::Integrity(
                "active conversation contains duplicate user turns".into(),
            ));
        }
        active_order.push(message.turn_id.clone());
    }
    let rows = connection.query_all_raw(sql(
        "SELECT t.id AS turn_id, t.user_message_id, t.user_commit_id, t.created_at AS turn_created_at, tc.run_id, tc.branch_id, tc.base_commit_id, tc.reply_output_key, tc.status AS candidate_status, tc.assistant_message_id, tc.candidate_commit_id, tc.projection_error_object_id, tc.created_at AS candidate_created_at, s.selected_run_id, c.context_id, b.context_id AS branch_context, r.context_id AS run_context, r.branch_id AS run_branch, r.status AS run_status, am.commit_id AS assistant_commit, cc.aggregate_id AS candidate_context, cc.lineage_key AS candidate_branch, pe.lifecycle AS error_lifecycle, pe.content_hash AS error_hash, pe.byte_size AS error_size, pe.inline_bytes AS error_bytes, EXISTS (SELECT 1 FROM content_object_refs ref WHERE ref.object_id = tc.projection_error_object_id AND ref.owner_kind = 'turn_candidate' AND ref.owner_id = tc.run_id AND ref.role = 'projection_error') AS error_ref FROM conversation_turns t JOIN conversations c ON c.id = t.conversation_id JOIN turn_candidates tc ON tc.turn_id = t.id JOIN context_branches b ON b.id = tc.branch_id JOIN graph_runs r ON r.id = tc.run_id LEFT JOIN conversation_selections s ON s.turn_id = t.id LEFT JOIN conversation_messages am ON am.id = tc.assistant_message_id LEFT JOIN version_commits cc ON cc.id = tc.candidate_commit_id AND cc.aggregate_kind = 'working_context' LEFT JOIN content_objects pe ON pe.id = tc.projection_error_object_id WHERE t.conversation_id = ? ORDER BY t.created_at, t.id, tc.created_at, tc.run_id",
        vec![conversation_id.into()],
    )).await?;
    let mut turns = HashMap::<String, TurnAccumulator>::new();
    for row in rows {
        let turn_id: String = row.try_get("", "turn_id")?;
        if !active.contains(&turn_id) {
            continue;
        }
        let selected_run_id: Option<String> = row.try_get("", "selected_run_id")?;
        let turn = ConversationTurnView {
            id: turn_id.clone(),
            conversation_id: conversation_id.into(),
            user_message_id: row.try_get("", "user_message_id")?,
            user_commit_id: row.try_get("", "user_commit_id")?,
            created_at: row.try_get("", "turn_created_at")?,
        };
        let status = parse_status(&row.try_get::<String>("", "candidate_status")?)?;
        let error = load_error(&row)?;
        validate_candidate(&row, &turn, status, error.is_some())?;
        let candidate = ConversationCandidateView {
            turn_id: turn_id.clone(),
            run_id: row.try_get("", "run_id")?,
            branch_id: row.try_get("", "branch_id")?,
            base_commit_id: row.try_get("", "base_commit_id")?,
            reply_output_key: row.try_get("", "reply_output_key")?,
            status,
            assistant_message_id: row.try_get("", "assistant_message_id")?,
            candidate_commit_id: row.try_get("", "candidate_commit_id")?,
            projection_error: error,
            created_at: row.try_get("", "candidate_created_at")?,
        };
        let entry = turns.entry(turn_id).or_insert_with(|| TurnAccumulator {
            turn: turn.clone(),
            selected_run_id: selected_run_id.clone(),
            candidates: Vec::new(),
        });
        if entry.turn != turn || entry.selected_run_id != selected_run_id {
            return Err(StorageError::Integrity(
                "conversation turn candidate rows disagree".into(),
            ));
        }
        entry.candidates.push(candidate);
    }
    active_order
        .into_iter()
        .map(|turn_id| {
            let turn = turns.remove(&turn_id).ok_or_else(|| {
                StorageError::Integrity("active conversation turn is missing".into())
            })?;
            if turn.selected_run_id.as_ref().is_some_and(|selected| {
                !turn
                    .candidates
                    .iter()
                    .any(|candidate| &candidate.run_id == selected)
            }) {
                return Err(StorageError::Integrity(
                    "conversation selection candidate is missing".into(),
                ));
            }
            Ok(ConversationTurnDetailView {
                turn: turn.turn,
                selected_run_id: turn.selected_run_id,
                candidates: turn.candidates,
            })
        })
        .collect()
}

fn parse_status(value: &str) -> StorageResult<TurnCandidateStatus> {
    match value {
        "running" => Ok(TurnCandidateStatus::Running),
        "ready" => Ok(TurnCandidateStatus::Ready),
        "failed" => Ok(TurnCandidateStatus::Failed),
        "cancelled" => Ok(TurnCandidateStatus::Cancelled),
        "projection_conflicted" => Ok(TurnCandidateStatus::ProjectionConflicted),
        "projection_failed" => Ok(TurnCandidateStatus::ProjectionFailed),
        "projection_abandoned" => Ok(TurnCandidateStatus::ProjectionAbandoned),
        _ => Err(StorageError::Integrity("invalid candidate status".into())),
    }
}
