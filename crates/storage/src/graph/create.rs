use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{canonical, graph::GraphDraft};

use crate::{SqliteStore, StorageError, StorageResult};

use super::{CreateGraphCommand, GraphView, helpers::*};

impl SqliteStore {
    pub async fn create_graph(&self, command: CreateGraphCommand) -> StorageResult<GraphView> {
        let name = command.name.trim();
        if name.is_empty() || name.len() > 200 || command.idempotency_key.is_empty() {
            return Err(StorageError::Integrity(
                "invalid graph create command".into(),
            ));
        }
        let scope = "workspace:local:graphs:create";
        let digest = canonical::hash(&json!({"command":"create_graph","name":name}))?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let id = receipt
                .resource_id
                .ok_or_else(|| StorageError::Integrity("receipt has no graph id".into()))?;
            let graph = load_graph(&transaction, &id).await?;
            transaction.commit().await?;
            return Ok(graph);
        }
        let id = new_id("graph");
        let revision_token = new_id("draftrev");
        let now = now_ms();
        transaction.execute(sql(
            "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, created_at) VALUES (?, ?, ?, 'create_graph', 'graph', ?, 'pending', ?)",
            vec![scope.into(), command.idempotency_key.clone().into(), digest.into(), id.clone().into(), now.into()],
        )).await?;
        transaction
            .execute(sql(
                "INSERT INTO graphs (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
                vec![id.clone().into(), name.into(), now.into(), now.into()],
            ))
            .await?;
        let draft = GraphDraft {
            graph_id: id.clone(),
            name: Some(name.into()),
            nodes: vec![],
            edges: vec![],
            run_input_schema: None,
            output_contract: vec![],
            limits: None,
        };
        let document = canonical::to_string(&draft)?;
        transaction.execute(sql(
            "INSERT INTO graph_drafts (graph_id, document_json, revision_token, updated_at) VALUES (?, ?, ?, ?)",
            vec![id.clone().into(), document.into(), revision_token.into(), now.into()],
        )).await?;
        transaction.execute(sql(
            "UPDATE application_command_receipts SET status = 'completed', completed_at = ? WHERE scope = ? AND idempotency_key = ?",
            vec![now.into(), scope.into(), command.idempotency_key.into()],
        )).await?;
        transaction.commit().await?;
        load_graph(&self.db, &id).await
    }
}
