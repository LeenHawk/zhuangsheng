use zhuangsheng_core::{
    application::conversation::{
        RegenerateConversationCandidateCommand, RegenerateConversationCandidateResult,
        ResolveCandidateProjectionCommand, ResolveCandidateProjectionResult,
        SelectConversationCandidateCommand, SubmitConversationTurnCommand,
        SubmitConversationTurnResult,
    },
    conversation::{
        ConversationListView, ConversationSelectionView, ConversationTimelineView,
        ConversationTurnDetailView, ConversationView,
    },
    runtime::ContextBranchView,
};

use crate::{CommandResult, TauriAdapter};

impl TauriAdapter {
    pub async fn get_conversation(&self, id: &str) -> CommandResult<ConversationView> {
        Ok(self.conversation.get_conversation(id).await?)
    }

    pub async fn list_conversations(&self) -> CommandResult<ConversationListView> {
        Ok(self.conversation.list_conversations().await?)
    }

    pub async fn get_conversation_timeline(
        &self,
        id: &str,
    ) -> CommandResult<ConversationTimelineView> {
        Ok(self.conversation.get_conversation_timeline(id).await?)
    }

    pub async fn get_turn_candidates(
        &self,
        turn_id: &str,
    ) -> CommandResult<ConversationTurnDetailView> {
        Ok(self.conversation.get_turn_candidates(turn_id).await?)
    }

    pub async fn submit_conversation_turn(
        &self,
        command: SubmitConversationTurnCommand,
    ) -> CommandResult<SubmitConversationTurnResult> {
        Ok(self.conversation.submit_turn(command).await?)
    }

    pub async fn select_conversation_candidate(
        &self,
        command: SelectConversationCandidateCommand,
    ) -> CommandResult<ConversationSelectionView> {
        Ok(self.conversation.select_candidate(command).await?)
    }

    pub async fn regenerate_conversation_candidate(
        &self,
        command: RegenerateConversationCandidateCommand,
    ) -> CommandResult<RegenerateConversationCandidateResult> {
        Ok(self.conversation.regenerate_candidate(command).await?)
    }

    pub async fn resolve_candidate_projection(
        &self,
        command: ResolveCandidateProjectionCommand,
    ) -> CommandResult<ResolveCandidateProjectionResult> {
        Ok(self
            .conversation
            .resolve_candidate_projection(command)
            .await?)
    }

    pub async fn list_context_branches(
        &self,
        context_id: &str,
    ) -> CommandResult<Vec<ContextBranchView>> {
        Ok(self.context.list_context_branches(context_id).await?)
    }
}
