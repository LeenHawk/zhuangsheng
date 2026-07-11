use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        memory::{
            ApplyMemoryProposalCommand, DecideMemoryProposalCommand, MemorySearchCommand,
            MemorySearchView, MemoryService, ProposeMemoryChangeCommand,
        },
    },
    memory::{LongTermMemoryRecordView, MemoryChangeProposalView},
};

use crate::SqliteStore;

use super::query::load_record;

#[async_trait]
impl MemoryService for SqliteStore {
    async fn propose_memory_change(
        &self,
        command: ProposeMemoryChangeCommand,
    ) -> Result<MemoryChangeProposalView, ApplicationError> {
        SqliteStore::propose_memory_change(self, command)
            .await
            .map_err(Into::into)
    }

    async fn decide_memory_proposal(
        &self,
        command: DecideMemoryProposalCommand,
    ) -> Result<MemoryChangeProposalView, ApplicationError> {
        SqliteStore::decide_memory_proposal(self, command)
            .await
            .map_err(Into::into)
    }

    async fn apply_memory_proposal(
        &self,
        command: ApplyMemoryProposalCommand,
    ) -> Result<MemoryChangeProposalView, ApplicationError> {
        SqliteStore::apply_memory_proposal(self, command)
            .await
            .map_err(Into::into)
    }

    async fn get_memory_record(
        &self,
        memory_id: &str,
    ) -> Result<LongTermMemoryRecordView, ApplicationError> {
        load_record(&self.db, memory_id).await.map_err(Into::into)
    }

    async fn search_memory(
        &self,
        command: MemorySearchCommand,
    ) -> Result<MemorySearchView, ApplicationError> {
        SqliteStore::search_memory(self, command)
            .await
            .map_err(Into::into)
    }
}

impl SqliteStore {
    pub async fn get_memory_record(
        &self,
        memory_id: &str,
    ) -> crate::StorageResult<LongTermMemoryRecordView> {
        load_record(&self.db, memory_id).await
    }
}
