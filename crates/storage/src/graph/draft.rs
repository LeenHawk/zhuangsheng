use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{canonical, graph::GraphDraft};

use crate::{SqliteStore, StorageError, StorageResult};

use super::{GraphDraftView, UpdateGraphDraftCommand, helpers::*};

impl SqliteStore {
    pub async fn get_graph_draft(&self, graph_id: &str) -> StorageResult<GraphDraftView> {
        load_draft(&self.db, graph_id).await
    }

    pub async fn update_graph_draft(
        &self,
        command: UpdateGraphDraftCommand,
    ) -> StorageResult<GraphDraftView> {
        if command.graph_id != command.document.graph_id || command.idempotency_key.is_empty() {
            return Err(StorageError::InvalidArgument(
                "draft graph identity mismatch".into(),
            ));
        }
        let document_json = canonical::to_string(&command.document)?;
        let digest = canonical::hash(&json!({
            "command": "update_graph_draft",
            "graphId": command.graph_id,
            "expectedRevisionToken": command.expected_revision_token,
            "documentHash": canonical::hash(&command.document)?,
        }))?;
        let scope = format!("workspace:local:graphs:{}:draft", command.graph_id);
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, &scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let object = receipt.result_object_id.ok_or_else(|| {
                StorageError::Integrity("draft receipt has no result object".into())
            })?;
            let result = load_object_json(&transaction, &object).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let current = load_draft(&transaction, &command.graph_id).await?;
        if current.revision_token != command.expected_revision_token {
            return Err(StorageError::Conflict("graph_draft_revision"));
        }
        let token = new_id("draftrev");
        let now = now_ms();
        transaction.execute_raw(sql(
            "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, created_at) VALUES (?, ?, ?, 'update_graph_draft', 'graph', ?, 'pending', ?)",
            vec![scope.clone().into(), command.idempotency_key.clone().into(), digest.into(), command.graph_id.clone().into(), now.into()],
        )).await?;
        let updated = transaction.execute_raw(sql(
            "UPDATE graph_drafts SET document_json = ?, revision_token = ?, updated_at = ? WHERE graph_id = ? AND revision_token = ?",
            vec![document_json.into(), token.clone().into(), now.into(), command.graph_id.clone().into(), command.expected_revision_token.into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("graph_draft_revision"));
        }
        transaction
            .execute_raw(sql(
                "UPDATE graphs SET name = ?, updated_at = ? WHERE id = ?",
                vec![
                    command
                        .document
                        .name
                        .clone()
                        .unwrap_or_else(|| current.document.name.unwrap_or_default())
                        .into(),
                    now.into(),
                    command.graph_id.clone().into(),
                ],
            ))
            .await?;
        let result = GraphDraftView {
            graph_id: command.graph_id,
            document: command.document,
            revision_token: token,
            updated_at: now,
        };
        let object_id = put_inline_object(&transaction, &canonical::to_vec(&result)?, now).await?;
        transaction.execute_raw(sql(
            "UPDATE application_command_receipts SET status = 'completed', result_object_id = ?, completed_at = ? WHERE scope = ? AND idempotency_key = ?",
            vec![object_id.into(), now.into(), scope.into(), command.idempotency_key.into()],
        )).await?;
        transaction.commit().await?;
        Ok(result)
    }
}

pub async fn load_draft<C: ConnectionTrait>(
    connection: &C,
    graph_id: &str,
) -> StorageResult<GraphDraftView> {
    let row = connection.query_one_raw(sql(
        "SELECT graph_id, document_json, revision_token, updated_at FROM graph_drafts WHERE graph_id = ?",
        vec![graph_id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "graph_draft", id: graph_id.into() })?;
    let document_json: String = row.try_get("", "document_json")?;
    let document: GraphDraft = serde_json::from_str(&document_json)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    if document.graph_id != graph_id {
        return Err(StorageError::Integrity(
            "stored draft graph identity mismatch".into(),
        ));
    }
    Ok(GraphDraftView {
        graph_id: graph_id.into(),
        document,
        revision_token: row.try_get("", "revision_token")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}
