use async_trait::async_trait;
use zhuangsheng_core::application::{ApplicationError, graph::*};

use crate::SqliteStore;

#[async_trait]
impl GraphService for SqliteStore {
    async fn create_graph(
        &self,
        command: CreateGraphCommand,
    ) -> Result<CreateGraphResult, ApplicationError> {
        SqliteStore::create_graph(self, command)
            .await
            .map_err(Into::into)
    }

    async fn list_graphs(&self) -> Result<Vec<GraphView>, ApplicationError> {
        SqliteStore::list_graphs(self).await.map_err(Into::into)
    }

    async fn get_graph_draft(&self, graph_id: &str) -> Result<GraphDraftView, ApplicationError> {
        SqliteStore::get_graph_draft(self, graph_id)
            .await
            .map_err(Into::into)
    }

    async fn update_graph_draft(
        &self,
        command: UpdateGraphDraftCommand,
    ) -> Result<GraphDraftView, ApplicationError> {
        SqliteStore::update_graph_draft(self, command)
            .await
            .map_err(Into::into)
    }

    async fn apply_graph(
        &self,
        command: ApplyGraphCommand,
    ) -> Result<GraphRevisionView, ApplicationError> {
        SqliteStore::apply_graph(self, command)
            .await
            .map_err(Into::into)
    }

    async fn get_graph_revision(
        &self,
        revision_id: &str,
    ) -> Result<GraphRevisionView, ApplicationError> {
        SqliteStore::get_graph_revision(self, revision_id)
            .await
            .map_err(Into::into)
    }

    async fn get_graph_revision_for_graph(
        &self,
        graph_id: &str,
        revision_id: &str,
    ) -> Result<GraphRevisionView, ApplicationError> {
        SqliteStore::get_graph_revision_for_graph(self, graph_id, revision_id)
            .await
            .map_err(Into::into)
    }
}
