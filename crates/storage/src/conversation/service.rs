use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        conversation::{
            ConversationService, CreateConversationCommand, RegenerateConversationCandidateCommand,
            RegenerateConversationCandidateResult, ResolveCandidateProjectionCommand,
            ResolveCandidateProjectionResult, SelectConversationCandidateCommand,
            SubmitConversationTurnCommand, SubmitConversationTurnResult,
            UpdateConversationRunProfileCommand,
        },
    },
    conversation::{
        ConversationListView, ConversationRunProfile, ConversationSelectionView,
        ConversationTimelineView, ConversationTurnDetailView, ConversationView,
    },
};

use crate::{SqliteStore, graph::helpers::now_ms};

use super::{
    read::load_conversation, read_list::load_conversations, read_timeline::load_timeline,
    read_turn::load_turn_candidates,
};

#[async_trait]
impl ConversationService for SqliteStore {
    async fn create_conversation(
        &self,
        command: CreateConversationCommand,
    ) -> Result<ConversationView, ApplicationError> {
        self.create_conversation_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn get_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationView, ApplicationError> {
        load_conversation(&self.db, conversation_id)
            .await
            .map_err(Into::into)
    }

    async fn list_conversations(&self) -> Result<ConversationListView, ApplicationError> {
        load_conversations(&self.db).await.map_err(Into::into)
    }

    async fn get_conversation_timeline(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationTimelineView, ApplicationError> {
        load_timeline(&self.db, conversation_id)
            .await
            .map_err(Into::into)
    }

    async fn get_turn_candidates(
        &self,
        turn_id: &str,
    ) -> Result<ConversationTurnDetailView, ApplicationError> {
        load_turn_candidates(&self.db, turn_id)
            .await
            .map_err(Into::into)
    }

    async fn update_run_profile(
        &self,
        command: UpdateConversationRunProfileCommand,
    ) -> Result<ConversationRunProfile, ApplicationError> {
        self.update_conversation_run_profile_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn submit_turn(
        &self,
        command: SubmitConversationTurnCommand,
    ) -> Result<SubmitConversationTurnResult, ApplicationError> {
        self.submit_conversation_turn_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn select_candidate(
        &self,
        command: SelectConversationCandidateCommand,
    ) -> Result<ConversationSelectionView, ApplicationError> {
        self.select_conversation_candidate_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn regenerate_candidate(
        &self,
        command: RegenerateConversationCandidateCommand,
    ) -> Result<RegenerateConversationCandidateResult, ApplicationError> {
        self.regenerate_conversation_candidate_at(command, now_ms())
            .await
            .map_err(Into::into)
    }

    async fn resolve_candidate_projection(
        &self,
        command: ResolveCandidateProjectionCommand,
    ) -> Result<ResolveCandidateProjectionResult, ApplicationError> {
        self.resolve_candidate_projection_at(command, now_ms())
            .await
            .map_err(Into::into)
    }
}

impl SqliteStore {
    pub async fn list_conversation_views(&self) -> crate::StorageResult<ConversationListView> {
        load_conversations(&self.db).await
    }

    pub async fn get_conversation_view(
        &self,
        conversation_id: &str,
    ) -> crate::StorageResult<ConversationView> {
        load_conversation(&self.db, conversation_id).await
    }

    pub async fn get_conversation_timeline_view(
        &self,
        conversation_id: &str,
    ) -> crate::StorageResult<ConversationTimelineView> {
        load_timeline(&self.db, conversation_id).await
    }
}
